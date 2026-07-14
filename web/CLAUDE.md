# EdgionCenter Dashboard — AI Agent Project Guide

## Project Overview

The EdgionCenter dashboard is the web management UI for EdgionCenter, built on React 18 +
TypeScript + Ant Design 5 + Vite 5. It talks to the Center Admin API (port 12201) over REST
to manage multi-cluster federation and the per-controller resources proxied through Center.

**Stack:** React 18, TypeScript 5, Ant Design 5, Vite 5, React Router 6, React Query 5,
Monaco Editor, Zod, Axios.

**Dev server:** `npm run dev` (port 5173, proxies `/api` to `localhost:12201`).

## Knowledge System

When a task requires project context, start from `skills/SKILL.md` and **load progressively on demand** — do not read everything at once.

### Skills Navigation Rules

1. **Progressive loading:** `skills/SKILL.md` → category SKILL.md → specific file. Load only the minimum subtree needed for the current task.
2. **Three-tier lookup:**
   - **Understand architecture** → `01-architecture/` — project structure, data flow, API layer
   - **Component patterns** → `02-patterns/` — list pages, editors, forms, YAML mode
   - **Resource development** → `03-resources/` — per-resource dev guides and Schema
3. **New resource page:** first read `02-patterns/` to understand existing patterns, then `03-resources/` for the target resource Schema, then reference the completed HTTPRoute/EdgionPlugins implementations.

## Core Development Patterns

### Adding a New Resource Management Page (Standard Flow)

Every new resource follows the pattern established by HTTPRoute and EdgionPlugins:

1. **Type definitions** `src/types/{resource}/index.ts`
   - TypeScript interfaces matching the backend YAML Schema
   - Export primary type and sub-types

2. **Utility functions** `src/utils/{resource}.ts`
   - `createEmpty{Resource}()` — create an empty object
   - `normalize{Resource}(raw)` — normalize data returned from the backend
   - `{resource}ToYaml(obj)` — convert object to YAML
   - `yamlTo{Resource}(str)` — convert YAML to object
   - Count / statistics helpers

3. **Editor component** `src/components/ResourceEditor/{Resource}/`
   - `{Resource}Editor.tsx` — Modal container (Form / YAML dual tabs)
   - `{Resource}Form.tsx` — form container
   - `sections/` — form sections split by feature

4. **List page** `src/pages/{Category}/{Resource}List.tsx`
   - Ant Design Table + search + bulk operations
   - React Query `useQuery` for data fetching
   - `useMutation` for CRUD

5. **Route registration** `src/App.tsx`
   - Add `<Route>` element

### API Call Conventions

- Namespaced resources: `resourceApi` (`src/api/resources.ts`)
- Cluster-scoped resources: `clusterResourceApi`
- Content-Type: `application/yaml` (create / update)
- React Query staleTime: 5 minutes

### Validation Conventions

- Zod schemas live under `src/schemas/`
- Validate with Zod before form submission
- Reuse regexes from `src/constants/gateway-api.ts` for DNS-1123 subdomains, hostnames, etc.

## Resource Scope Reference

> **Note:** Kind/apiVersion values are upstream Edgion facts; treat the upstream Schema as authoritative.

| Resource | Scope | API | Kind value | apiVersion |
|----------|-------|-----|------------|-----------|
| HTTPRoute | namespaced | resourceApi | `httproute` | gateway.networking.k8s.io/v1 |
| GRPCRoute | namespaced | resourceApi | `grpcroute` | gateway.networking.k8s.io/v1 |
| TCPRoute | namespaced | resourceApi | `tcproute` | gateway.networking.k8s.io/v1alpha2 |
| UDPRoute | namespaced | resourceApi | `udproute` | gateway.networking.k8s.io/v1alpha2 |
| TLSRoute | namespaced | resourceApi | `tlsroute` | gateway.networking.k8s.io/v1 |
| Gateway | namespaced | resourceApi | `gateway` | gateway.networking.k8s.io/v1 |
| GatewayClass | cluster | clusterResourceApi | `gatewayclass` | gateway.networking.k8s.io/v1 |
| Service | namespaced | resourceApi | `service` | v1 |
| EndpointSlice | namespaced | resourceApi | `endpointslice` | discovery.k8s.io/v1 |
| EdgionPlugins | namespaced | resourceApi | `edgionplugins` | edgion.io/v1 |
| EdgionStreamPlugins | namespaced | resourceApi | `edgionstreamplugins` | edgion.io/v1 |
| EdgionTls | namespaced | resourceApi | `edgiontls` | edgion.io/v1 |
| EdgionGatewayConfig | cluster | clusterResourceApi | `edgiongatewayconfig` | edgion.io/v1alpha1 |
| PluginMetaData | namespaced | resourceApi | `pluginmetadata` | edgion.io/v1 |
| Secret | namespaced | resourceApi | `secret` | v1 |
| BackendTLSPolicy | namespaced | resourceApi | `backendtlspolicy` | gateway.networking.k8s.io/v1alpha3 |
| ReferenceGrant | namespaced | resourceApi | `referencegrant` | gateway.networking.k8s.io/v1 |
| LinkSys | namespaced | resourceApi | `linksys` | edgion.io/v1 |
| EdgionAcme | namespaced | resourceApi | `edgionacme` | edgion.io/v1 |

**Note:** `edgionstreamplugins`, `referencegrant`, and `edgionacme` must be added to the `ResourceKind` type in `src/api/types.ts`.

### API Response Format (unified since feature-04-06)

```typescript
// Standard response
interface ApiResponse<T> {
  success: boolean
  data?: T
  error?: string
}

// List response (added continue_token pagination support)
interface ListResponse<T> {
  success: boolean
  data?: T[]
  count: number
  continue_token?: string  // pagination token
  error?: string
}
```

### Center API endpoints

```
GET  /health                                    # liveness probe (dedicated probe listener :12200, not the Admin port)
GET  /ready                                     # readiness probe (dedicated probe listener :12200, not the Admin port)
GET  /api/v1/server-info                        # server info (Admin :12201; frontend uses the ready field to check health/readiness instead of relying on same-origin /health)
POST /api/v1/reload                             # reload all resources
GET  /api/v1/namespaced/{kind}                  # list all namespaced resources
GET  /api/v1/namespaced/{kind}/{namespace}      # list resources in a specific namespace
*    /api/v1/namespaced/{kind}/{ns}/{name}      # single resource CRUD
GET  /api/v1/cluster/{kind}                     # list cluster-scoped resources
*    /api/v1/cluster/{kind}/{name}              # single cluster resource CRUD
POST /api/v1/services/acme/{ns}/{name}/trigger  # manually trigger ACME certificate issuance
```

### Authentication

- Login page: `/login`, authenticated via httpOnly Cookie
- Auth API: `authApi` (`src/api/auth.ts`) — login / logout / me
- Session state: `sessionStorage` (`src/utils/auth.ts`) — token is NOT stored in localStorage
- Route guard: `RequireAuth` component (`src/App.tsx`)
- 401 handling: auto-redirect to `/login` (`src/api/client.ts`)

## Common Commands

```bash
# Development
npm run dev              # start dev server (port 5173)
npm run build            # TypeScript compile + Vite build
npm run lint             # ESLint check
npm run preview          # preview production build

# Backend (run from the EdgionCenter repo root)
cargo run -p edgion-center-standalone -- --config-file config/edgion-center.yaml
# Center Admin API: http://localhost:12201
```

## Directory Structure

```
src/
├── api/                  # HTTP client layer (Axios + generic CRUD)
├── components/
│   ├── Layout/           # MainLayout
│   ├── ResourceEditor/   # resource editors (one directory per resource type)
│   └── YamlEditor/       # Monaco YAML editor
├── constants/            # constants, enums, regexes, defaults
├── pages/                # page components (grouped by feature)
│   ├── Dashboard/
│   ├── Routes/           # HTTPRoute, GRPCRoute, ...
│   └── Plugins/          # EdgionPlugins, ...
├── schemas/              # Zod validation schemas
├── types/                # TypeScript type definitions
│   ├── gateway-api/      # Gateway API standard resource types
│   └── edgion-plugins/   # Edgion custom resource types
├── utils/                # utility functions (YAML conversion, validation, etc.)
├── App.tsx               # route definitions
└── main.tsx              # entry point (React / Router / Query / Ant Design)
```

## Internationalization (i18n) — Mandatory Rules

> **Never hard-code Chinese or bilingual mixed strings in components. All UI text must be output via `t()`.**

### Quick Usage

```typescript
import { useT } from '@/i18n'
const t = useT()  // call inside a React component function body

t('btn.create')                              // "Create"
t('col.name')                               // "Name"
t('msg.deleteOk')                           // "Deleted successfully"
t('modal.create', { resource: 'Gateway' })  // "Create Gateway"
t('msg.batchDeleteOk', { n: 5 })            // "5 resources deleted"
t('table.totalItems', { n: total })         // "Total: 42"
t('confirm.deleteMsg', { name })            // confirmation text with resource name
t('msg.createFailed', { err: e.message })   // failure message with error info
```

### Technical Terms Are Not Translated

`HTTPRoute`, `GRPCRoute`, `Gateway`, `EdgionTls`, `YAML`, `HTTP`, `HTTPS`, `TCP`, `Exact`, etc. are written as literals:
```typescript
`${t('btn.create')} Gateway`   // "Create Gateway"
```

### Rules for Adding New Keys

1. Check `skills/02-patterns/SKILL.md` (i18n quick reference) or `skills/02-patterns/04-i18n-rules.md` (full key list) first.
2. **Simultaneously** update `src/i18n/en.ts` and `src/i18n/zh.ts` — the keys in both files must be exactly in sync.
3. Translation file locations: `src/i18n/en.ts` (English, default), `src/i18n/zh.ts` (Chinese).

### Standard Pagination

```typescript
pagination={{ showTotal: (n) => t('table.totalItems', { n }) }}
```

### Standard Delete Confirmation

```typescript
Modal.confirm({
  title: t('confirm.deleteTitle'),
  content: t('confirm.deleteMsg', { name }),
  okText: t('confirm.okText'),
  okType: 'danger',
  cancelText: t('btn.cancel'),
  onOk: () => deleteMutation.mutate(...),
})
```

## Coding Conventions

- Component files use PascalCase; utility files use camelCase
- List page naming: `{Resource}List.tsx`; editor naming: `{Resource}Editor.tsx`
- Reference HTTPRouteList / HTTPRouteEditor patterns for new components
- Use Ant Design components; do not introduce additional UI libraries
- **All UI text goes through i18n (see i18n rules above)**
- Type definitions live in separate files — do not inline them in components
