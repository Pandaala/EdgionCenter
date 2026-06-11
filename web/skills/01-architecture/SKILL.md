---
name: dashboard-architecture
description: Edgion Controller 项目架构概览——目录结构、数据流、技术栈、路由、状态管理
---

# 项目架构

## 技术栈

| 层 | 技术 | 版本 |
|----|------|------|
| UI 框架 | React | 18.2 |
| 语言 | TypeScript | 5.2 |
| 构建 | Vite | 5.0 |
| UI 组件 | Ant Design | 5.12 |
| 路由 | React Router | 6.20 |
| 服务端状态 | React Query (TanStack) | 5.14 |
| 客户端状态 | Zustand | 4.4（已安装，暂未使用） |
| HTTP 客户端 | Axios | 1.6 |
| 代码编辑器 | Monaco Editor (@monaco-editor/react) | 4.6 |
| 验证 | Zod | 4.2 |
| YAML | js-yaml | 4.1 |
| 日期 | dayjs | 1.11 |
| 图可视化 | React Flow | 11.11 |
| 图布局 | @dagrejs/dagre | 3.0 |

## 数据流

```
┌─────────────────────────────────────────────────────────┐
│  React Component (页面/编辑器)                            │
│  ├── useQuery() 获取数据                                  │
│  ├── useMutation() 提交变更                               │
│  └── queryClient.invalidateQueries() 刷新缓存             │
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
              │ timeout: 30s │  错误拦截 + Ant Design message
              └──────┬──────┘
                     │
              ┌──────▼──────┐
              │ Vite Proxy   │  dev: localhost:5173 → localhost:12101
              └──────┬──────┘
                     │
              ┌──────▼──────┐
              │ Edgion       │
              │ Controller   │  port 12101
              │ Admin API    │
              └──────────────┘
```

## 路由结构

认证说明：所有业务路由都被 `RequireAuth` 组件包裹——未登录时自动重定向到 `/login`。登录态通过 `sessionStorage` 标记检测（不依赖 JS 可读 token，实际凭据为 httpOnly Cookie）。

```
/login                             → LoginPage（公开，无需认证）
/ (RequireAuth → MainLayout / CenterLayout)
├── /                              → Dashboard (OPS)
├── /user                          → UserDashboard (USER)
├── /topology                      → TopologyPage (资源拓扑可视化，React Flow)
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
│   ├── /list                      → ServiceList（只读）
│   └── /endpointslices            → EndpointSliceList（只读）
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

## 目录结构详解

```
src/
├── api/                        # API 层
│   ├── client.ts               # Axios 实例 + 拦截器（401 自动跳转 /login）
│   ├── auth.ts                 # authApi（login/logout/me）
│   ├── resources.ts            # 通用 CRUD（namespaced + cluster）
│   └── types.ts                # API 响应类型、K8sResource 基类、ResourceKind 枚举
│
├── components/                 # 可复用组件
│   ├── Layout/
│   │   └── MainLayout.tsx      # 主布局（侧边栏 + Header + Content）
│   ├── ResourceEditor/         # 资源编辑器（按资源类型分目录）
│   │   ├── HTTPRoute/          # HTTPRoute 编辑器（6 个文件）
│   │   ├── EdgionPlugins/      # EdgionPlugins 编辑器（4 个文件）
│   │   └── YamlEditor/         # 通用 YAML 编辑器组件
│   └── YamlEditor/             # （旧位置，与 ResourceEditor 下的合并）
│
├── constants/                  # 常量
│   └── gateway-api.ts          # 正则、枚举、默认值、双语验证消息
│
├── pages/                      # 页面
│   ├── Login/                  # LoginPage（公开路由，不走 RequireAuth）
│   ├── Dashboard/              # OPS Dashboard + User Dashboard
│   ├── Topology/               # 资源拓扑可视化（React Flow + dagre）
│   │   ├── TopologyPage.tsx    # 主页面
│   │   ├── hooks/              # useTopologyData（数据获取 + 图构建）
│   │   └── components/         # Canvas、Legend、Drawer、nodes/（6 种节点）、layout/
│   ├── Routes/                 # HTTPRoute/GRPCRoute/TCPRoute/UDPRoute/TLSRoute
│   ├── Plugins/                # EdgionPlugins/StreamPlugins/PluginMetaData
│   └── ...                     # Infrastructure, Security, System, Login
│
├── schemas/                    # Zod 验证 Schema
│   └── gateway-api/            # HTTPRoute 等 Schema
│
├── types/                      # TypeScript 类型
│   ├── gateway-api/            # 标准 Gateway API 资源类型
│   │   ├── httproute.ts        # HTTPRoute 完整类型定义
│   │   ├── backend.ts          # BackendRef 等
│   │   └── common.ts           # K8s 通用类型
│   └── edgion-plugins/         # Edgion 自定义资源类型
│       └── index.ts            # EdgionPlugins 类型
│
├── utils/                      # 工具函数
│   ├── auth.ts                 # sessionStorage 登录态标记（setLoggedIn/clearLoggedIn/isLoggedIn）
│   ├── httproute.ts            # HTTPRoute YAML ↔ 对象转换
│   ├── edgionplugins.ts        # EdgionPlugins YAML ↔ 对象转换
│   └── validation.ts           # 通用验证工具
│
├── App.tsx                     # 路由定义
└── main.tsx                    # 入口
```

## 详细文件

- [02-api-layer.md](02-api-layer.md) — API 层设计、resourceApi/clusterResourceApi 详解、错误处理
