---
name: api-layer
description: Edgion Center API layer design — Axios client, resourceApi/clusterResourceApi, error handling
---

# API Layer Design

## authApi (Authentication)

| Method | Path | Description |
|------|------|------|
| `authApi.login(req)` | POST `auth/login` | Login; backend sets httpOnly Cookie |
| `authApi.logout()` | POST `auth/logout` | Password-session cookie logout; propagates errors |
| `authApi.me()` | GET `auth/me` | Identity, permissions and provider-specific logout metadata |

### Cookie Authentication Pattern

The frontend does not store the token (no localStorage). After login, the backend sets `Set-Cookie: edgion_token=<jwt>; HttpOnly; SameSite=Strict`; the browser attaches it automatically.

- `src/utils/auth.ts` — `localStorage` login state flag (`setLoggedIn`/`clearLoggedIn`/`isLoggedIn`)
- `src/api/auth.ts` — authApi (login/logout/me)
- `src/pages/Login/LoginPage.tsx` — login page
- `src/App.tsx` — `RequireAuth` route guard

Center's top bar reads fresh `authProvider` from `/auth/me` before logout. Local
sessions use the password endpoint. OIDC sessions navigate to the configured
same-origin `logoutPath`, first clearing the Center cookie when
`localLogoutAvailable` is true. Missing external configuration and failed API
requests are surfaced without pretending that local flag removal ends the
external session. External navigation does not assert IdP-wide token revocation.
The login page checks an existing authenticated session before showing password
fields, including when both OIDC and password login are enabled. This accepts
an already authenticated proxy session and prevents the initial identity check
from racing a password submission.

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
  | 'edgiontls' | 'edgionplugins' | 'edgionconfigdata' | 'linksys'
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
