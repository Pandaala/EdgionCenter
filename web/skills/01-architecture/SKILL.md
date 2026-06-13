---
name: dashboard-architecture
description: Edgion Center project architecture overview — directory structure, data flow, tech stack, routing, state management
---

# Project Architecture

## Tech Stack

| Layer | Technology | Version |
|----|------|------|
| UI Framework | React | 18.2 |
| Language | TypeScript | 5.2 |
| Build | Vite | 5.0 |
| UI Components | Ant Design | 5.12 |
| Routing | React Router | 6.20 |
| Server State | React Query (TanStack) | 5.14 |
| Client State | Zustand | 4.4 (installed, not yet in use) |
| HTTP Client | Axios | 1.6 |
| Code Editor | Monaco Editor (@monaco-editor/react) | 4.6 |
| Validation | Zod | 4.2 |
| YAML | js-yaml | 4.1 |
| Date | dayjs | 1.11 |
| Graph Visualization | React Flow | 11.11 |
| Graph Layout | @dagrejs/dagre | 3.0 |

## Data Flow

```
┌─────────────────────────────────────────────────────────┐
│  React Component (page/editor)                            │
│  ├── useQuery() fetch data                                │
│  ├── useMutation() submit changes                         │
│  └── queryClient.invalidateQueries() invalidate cache     │
└────────────────────┬────────────────────────────────────┘
                     │
              ┌──────▼──────┐
              │ React Query  │  staleTime: 5min, cacheTime: 10min
              │ Query Cache  │  refetchOnWindowFocus: false
              └──────┬──────┘
                     │
           ┌─────────▼─────────┐
           │  resourceApi /     │  src/api/resources.ts
           │  clusterResourceApi│  Content-Type: application/yaml
           └─────────┬─────────┘
                     │
              ┌──────▼──────┐
              │ Axios Client │  src/api/client.ts
              │ baseURL:     │  /api/v1
              │ timeout: 30s │  error interceptor + Ant Design message
              └──────┬──────┘
                     │
              ┌──────▼──────┐
              │ Vite Proxy   │  dev: localhost:5173 → localhost:12201
              └──────┬──────┘
                     │
              ┌──────▼──────┐
              │ Edgion       │
              │ Center       │  port 12201
              │ Admin API    │
              └──────────────┘
```

## Route Structure

Auth note: all business routes are wrapped by the `RequireAuth` component — unauthenticated users are automatically redirected to `/login`. Login state is tracked via a `sessionStorage` flag (no JS-readable token; actual credentials are stored in an httpOnly Cookie).

```
/login                             → LoginPage (public, no auth required)
/ (RequireAuth → MainLayout / CenterLayout)
├── /                              → Dashboard (OPS)
├── /user                          → UserDashboard (USER)
├── /topology                      → TopologyPage (resource topology visualization, React Flow)
├── /routes
│   ├── /http                      → HTTPRouteList
│   ├── /grpc                      → GRPCRouteList
│   ├── /tcp                       → TCPRouteList
│   ├── /udp                       → UDPRouteList
│   └── /tls                       → TLSRouteList
├── /infrastructure
│   ├── /gateways                  → GatewayList
│   ├── /gatewayclasses            → GatewayClassList
│   └── /referencegrants           → ReferenceGrantList
├── /services
│   ├── /list                      → ServiceList (read-only)
│   └── /endpointslices            → EndpointSliceList (read-only)
├── /security
│   ├── /tls                       → EdgionTlsList
│   └── /backendtls                → BackendTLSPolicyList
├── /plugins
│   ├── /                          → EdgionPluginsList
│   ├── /stream                    → EdgionStreamPluginsList
│   └── /metadata                  → PluginMetaDataList
└── /system
    ├── /config                    → EdgionGatewayConfigPage
    ├── /linksys                   → LinkSysList
    └── /acme                      → EdgionAcmeList
```

## Directory Structure

```
src/
├── api/                        # API layer
│   ├── client.ts               # Axios instance + interceptors (401 auto-redirect to /login)
│   ├── auth.ts                 # authApi (login/logout/me)
│   ├── resources.ts            # generic CRUD (namespaced + cluster)
│   └── types.ts                # API response types, K8sResource base class, ResourceKind enum
│
├── components/                 # reusable components
│   ├── Layout/
│   │   └── MainLayout.tsx      # main layout (sidebar + Header + Content)
│   ├── ResourceEditor/         # resource editors (one directory per resource type)
│   │   ├── HTTPRoute/          # HTTPRoute editor (6 files)
│   │   ├── EdgionPlugins/      # EdgionPlugins editor (4 files)
│   │   └── YamlEditor/         # generic YAML editor component
│   └── YamlEditor/             # (legacy location, merging with ResourceEditor/)
│
├── constants/                  # constants
│   └── gateway-api.ts          # regexes, enums, defaults, bilingual validation messages
│
├── pages/                      # pages
│   ├── Login/                  # LoginPage (public route, bypasses RequireAuth)
│   ├── Dashboard/              # OPS Dashboard + User Dashboard
│   ├── Topology/               # resource topology visualization (React Flow + dagre)
│   │   ├── TopologyPage.tsx    # main page
│   │   ├── hooks/              # useTopologyData (data fetching + graph construction)
│   │   └── components/         # Canvas, Legend, Drawer, nodes/ (6 node types), layout/
│   ├── Routes/                 # HTTPRoute/GRPCRoute/TCPRoute/UDPRoute/TLSRoute
│   ├── Plugins/                # EdgionPlugins/StreamPlugins/PluginMetaData
│   └── ...                     # Infrastructure, Security, System, Login
│
├── schemas/                    # Zod validation schemas
│   └── gateway-api/            # HTTPRoute and other schemas
│
├── types/                      # TypeScript types
│   ├── gateway-api/            # standard Gateway API resource types
│   │   ├── httproute.ts        # complete HTTPRoute type definitions
│   │   ├── backend.ts          # BackendRef and others
│   │   └── common.ts           # common K8s types
│   └── edgion-plugins/         # Edgion custom resource types
│       └── index.ts            # EdgionPlugins types
│
├── utils/                      # utility functions
│   ├── auth.ts                 # sessionStorage login state flag (setLoggedIn/clearLoggedIn/isLoggedIn)
│   ├── httproute.ts            # HTTPRoute YAML ↔ object conversion
│   ├── edgionplugins.ts        # EdgionPlugins YAML ↔ object conversion
│   └── validation.ts           # generic validation utilities
│
├── App.tsx                     # route definitions
└── main.tsx                    # entry point
```

## Detailed Files

- [02-api-layer.md](02-api-layer.md) — API layer design, resourceApi/clusterResourceApi details, error handling
