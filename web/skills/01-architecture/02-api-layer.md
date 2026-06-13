---
name: api-layer
description: Edgion Center API layer design — Axios client, resourceApi/clusterResourceApi, error handling
---

# API Layer Design

## authApi (Authentication)

| Method | Path | Description |
|------|------|------|
| `authApi.login(req)` | POST `auth/login` | Login; backend sets httpOnly Cookie |
| `authApi.logout()` | POST `auth/logout` | Logout; clears Cookie |
| `authApi.me()` | GET `auth/me` | Get current user info |

### Cookie Authentication Pattern

The frontend does not store the token (no localStorage). After login, the backend sets `Set-Cookie: edgion_token=<jwt>; HttpOnly; SameSite=Strict`; the browser attaches it automatically.

- `src/utils/auth.ts` — `sessionStorage` login state flag (`setLoggedIn`/`clearLoggedIn`/`isLoggedIn`)
- `src/api/auth.ts` — authApi (login/logout/me)
- `src/pages/Login/LoginPage.tsx` — login page
- `src/App.tsx` — `RequireAuth` route guard

## Dual API Clients

Select based on resource scope:

### resourceApi (Namespaced Resources)

```typescript
// path pattern: /api/v1/namespaced/{kind}/{namespace}/{name}
resourceApi.listAll<T>(kind)                    // GET /namespaced/{kind}
resourceApi.list<T>(kind, namespace)            // GET /namespaced/{kind}/{namespace}
resourceApi.get<T>(kind, namespace, name)       // GET /namespaced/{kind}/{ns}/{name}
resourceApi.create<T>(kind, namespace, resource) // POST + Content-Type: application/yaml
resourceApi.update<T>(kind, namespace, name, resource) // PUT + Content-Type: application/yaml
resourceApi.delete(kind, namespace, name)       // DELETE
resourceApi.batchDelete(kind, resources[])      // parallel DELETE
```

### clusterResourceApi (Cluster-Scoped Resources)

```typescript
// path pattern: /api/v1/cluster/{kind}/{name}
clusterResourceApi.listAll<T>(kind)            // GET /cluster/{kind}
clusterResourceApi.get<T>(kind, name)          // GET /cluster/{kind}/{name}
clusterResourceApi.create<T>(kind, resource)    // POST
clusterResourceApi.update<T>(kind, name, resource) // PUT
clusterResourceApi.delete(kind, name)          // DELETE
```

## ResourceKind Type

Defined in `src/api/types.ts`. When adding a new resource, add the new kind value here:

```typescript
export type ResourceKind =
  | 'httproute' | 'grpcroute' | 'tcproute' | 'udproute' | 'tlsroute'
  | 'service' | 'endpointslice'
  | 'edgiontls' | 'edgionplugins' | 'pluginmetadata' | 'linksys'
  | 'secret' | 'gatewayclass' | 'edgiongatewayconfig' | 'gateway'
```

## YAML Serialization Convention

Create and update operations send YAML format:
- `resourceApi.create/update` accepts `T | string`
- If an object is passed, it is serialized internally with `yaml.dump()`
- Request header sets `Content-Type: application/yaml`

## Error Handling

Axios interceptors automatically handle common error codes:
- 401 Unauthorized → auto-redirect to `/login` (token expired or not logged in)
- 409 Conflict → resource already exists
- 404 Not Found → resource not found
- 400 Bad Request → invalid request parameters (shows backend message)
- 500/503 → server error
- All errors are displayed via `message.error()`

## React Query Integration Pattern

```typescript
// list page query
const { data, isLoading, refetch } = useQuery({
  queryKey: [kind],
  queryFn: () => resourceApi.listAll<T>(kind),
})

// create/update Mutation
const createMutation = useMutation({
  mutationFn: ({ namespace, yamlContent }: { namespace: string; yamlContent: string }) =>
    resourceApi.create(kind, namespace, yamlContent),
  onSuccess: () => {
    message.success('Created successfully')
    queryClient.invalidateQueries({ queryKey: [kind] })
  },
})
```
