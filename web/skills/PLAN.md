---
name: development-plan
description: Edgion Controller 前端开发总体规划——基于 feature-04-06 分支用户文档，6 Phase 详细任务分解
---

# Edgion Controller 前端开发计划

> 基于 Edgion `feature-04-06` 分支的用户文档和 API 定义

## 当前状态

| 功能 | 状态 | 代码量 |
|------|------|--------|
| 项目骨架（Vite + React + Ant Design + Router + Query） | ✅ 完成 | — |
| MainLayout（侧边栏 + Header） | ✅ 完成 | 131 行 |
| API 层（Axios + resourceApi + clusterResourceApi） | ✅ 完成 | 133 行 |
| 类型系统（K8sResource, ResourceKind） | ✅ 完成 | 53 行 |
| YamlEditor（Monaco 包装） | ✅ 完成 | 123 行 |
| Dashboard 页面 | ⚠️ 部分（Mock 数据） | 87 行 |
| HTTPRoute CRUD | ✅ 完成 | ~1000 行 |
| EdgionPlugins CRUD | ✅ 完成 | ~700 行 |
| **总计** | — | **~4,749 行** |

## Phase 总览

```
Phase 0  基础重构 + API 层更新
   ↓
Phase 1  路由类资源（GRPCRoute, TCPRoute, UDPRoute, TLSRoute）
   ↓
Phase 2  基础设施（Gateway, GatewayClass, Service, EndpointSlice, ReferenceGrant）
   ↓
Phase 3  安全 & TLS（EdgionTls, Secret, BackendTLSPolicy）
   ↓
Phase 4  插件扩展（EdgionStreamPlugins, PluginMetaData, LinkSys）
   ↓
Phase 5  系统配置（EdgionGatewayConfig, EdgionAcme）
   ↓
Phase 6  Dashboard 增强 & 体验优化
```

---

## Phase 0 — 基础重构 + API 层更新

**目标**：更新 API 层适配 feature-04-06 变更，提取公共组件。

### 0.1 API 层更新

- `src/api/types.ts` — 添加缺失的 ResourceKind 值：`edgionstreamplugins`, `referencegrant`, `edgionacme`
- `src/api/types.ts` — ListResponse 新增 `continue_token?: string` 分页支持
- 确认 `/health`, `/ready`, `/api/v1/server-info` 端点可用

### 0.2 提取公共表单区段组件

从 HTTPRoute 编辑器提取到 `src/components/ResourceEditor/common/`：

| 组件 | 来源 | 复用方 |
|------|------|--------|
| `MetadataSection.tsx` | HTTPRoute | 所有资源 |
| `ParentRefsSection.tsx` | HTTPRoute | 路由类、EdgionTls、Gateway |
| `HostnamesSection.tsx` | HTTPRoute | HTTPRoute, GRPCRoute, TLSRoute, EdgionTls |
| `BackendRefsEditor.tsx` | HTTPRoute | 所有路由类资源 |
| `HeaderMatchEditor.tsx` | HTTPRoute | HTTPRoute, GRPCRoute |
| `AnnotationsEditor.tsx` | 新建 | TCPRoute, TLSRoute, Gateway |

### 0.3 创建简化编辑器模板

`SimpleResourceEditor.tsx` — 只有 YAML 编辑器（无 Form 标签）的通用 Modal。
适用于：Service, EndpointSlice, GatewayClass, PluginMetaData, BackendTLSPolicy, ReferenceGrant

### 0.4 创建 useResourceList Hook

封装列表页通用逻辑：React Query 查询 + 搜索过滤 + 批量删除 + 编辑器状态管理

---

## Phase 1 — 路由类资源

**依赖**：Phase 0

### 1.1 TCPRoute & UDPRoute（最简单，先做验证模式）

**TCPRoute 特有字段**：annotations（edgion.io/edgion-stream-plugins, proxy-protocol, max-connect-retries）
**UDPRoute**：无特有 annotations

文件清单：
```
src/types/gateway-api/tcproute.ts
src/types/gateway-api/udproute.ts
src/utils/tcproute.ts
src/utils/udproute.ts
src/components/ResourceEditor/StreamRoute/
  ├── StreamRouteEditor.tsx            # 通用 TCP/UDP/TLS 编辑器
  └── StreamRouteForm.tsx              # metadata + annotations + parentRefs + backendRefs
src/pages/Routes/TCPRouteList.tsx
src/pages/Routes/UDPRouteList.tsx
```

表单区段（全部复用公共组件）：
- MetadataSection + AnnotationsEditor（TCPRoute 有三个 Edgion annotations）
- ParentRefsSection
- BackendRefsEditor

### 1.2 TLSRoute

复用 StreamRoute 编辑器（比 TCPRoute 多 hostnames）。

```
src/types/gateway-api/tlsroute.ts
src/utils/tlsroute.ts
src/pages/Routes/TLSRouteList.tsx
```

apiVersion: `gateway.networking.k8s.io/v1`（从 v1alpha3 晋升）

表单区段：MetadataSection + AnnotationsEditor + ParentRefsSection + HostnamesSection + BackendRefsEditor

### 1.3 GRPCRoute

```
src/types/gateway-api/grpcroute.ts
src/utils/grpcroute.ts
src/schemas/gateway-api/grpcroute.ts
src/components/ResourceEditor/GRPCRoute/
  ├── GRPCRouteEditor.tsx
  ├── GRPCRouteForm.tsx
  └── sections/
      ├── GRPCRulesSection.tsx
      └── GRPCMethodMatchEditor.tsx    # 新组件：service + method + type
src/pages/Routes/GRPCRouteList.tsx
```

关键差异：
- matches 用 `method`（service/method/type）替代 `path`
- **不支持** RequestRedirect 和 URLRewrite filters
- `retry.codes` 是 gRPC status codes（0-16，runtime-ignored）
- 支持 gRPC-Web 自动检测

### 1.4 路由注册

- `src/App.tsx` — 添加 GRPCRoute, TCPRoute, UDPRoute, TLSRoute 路由
- MainLayout 菜单已预定义，无需修改

---

## Phase 2 — 基础设施资源

### 2.1 Gateway

最复杂的编辑器——核心是 Listener 管理。

```
src/types/gateway-api/gateway.ts
src/utils/gateway.ts
src/components/ResourceEditor/Gateway/
  ├── GatewayEditor.tsx
  ├── GatewayForm.tsx
  └── sections/
      ├── GatewayAnnotationsSection.tsx   # HTTP→HTTPS redirect, HTTP/2, StreamPlugins
      ├── ListenersSection.tsx            # Listener 列表管理
      └── ListenerEditor.tsx              # 单个 Listener 编辑
src/pages/Infrastructure/GatewayList.tsx
```

Listener 编辑器需按 protocol 条件渲染：
- HTTP: name + port + hostname + allowedRoutes
- HTTPS: + tls.mode + tls.certificateRefs + tls.options
- TCP/UDP: name + port + allowedRoutes
- TLS: + hostname + tls.mode(Passthrough)

Gateway annotations 编辑：
- `edgion.io/http-to-https-redirect` — 开关
- `edgion.io/https-redirect-port` — 端口
- `edgion.io/enable-http2` — 开关
- `edgion.io/edgion-stream-plugins` — 引用选择

Status 只读展示：addresses, conditions, listeners(attachedRoutes)

### 2.2 GatewayClass

集群级资源，SimpleResourceEditor + 简单列表页。

```
src/pages/Infrastructure/GatewayClassList.tsx
```

列表列：name, controllerName, parametersRef, description

### 2.3 Service（只读）

```
src/pages/Infrastructure/ServiceList.tsx
```

列表列：name, namespace, type, ports(Tags), selector

### 2.4 EndpointSlice（只读）

```
src/pages/Infrastructure/EndpointSliceList.tsx
```

列表列：name, namespace, 关联 service, endpoint 数量, ready 状态

### 2.5 ReferenceGrant

```
src/types/gateway-api/referencegrant.ts
src/utils/referencegrant.ts
src/pages/Infrastructure/ReferenceGrantList.tsx
```

表单：from（group + kind + namespace 列表）+ to（group + kind 列表）
列表列：name, namespace, from 类型/命名空间, to 类型

### 2.6 侧边栏 & 路由更新

MainLayout 菜单新增：
- 基础设施组下添加 Gateway, GatewayClass, ReferenceGrant
- 更新路由注册

---

## Phase 3 — 安全 & TLS 资源

### 3.1 EdgionTls

```
src/types/edgion-tls/index.ts
src/utils/edgiontls.ts
src/components/ResourceEditor/EdgionTls/
  ├── EdgionTlsEditor.tsx
  ├── EdgionTlsForm.tsx
  └── sections/
      ├── HostsSection.tsx              # 域名列表（支持通配符）
      ├── SecretRefSection.tsx          # 证书引用选择器
      ├── ClientAuthSection.tsx         # mTLS（mode 条件渲染 caSecretRef/SAN/CN）
      └── TlsConfigSection.tsx          # minTlsVersion + cipherSuites
src/pages/Security/EdgionTlsList.tsx
```

clientAuth.mode 决定表单可见性：
- Terminate（默认）：无额外配置
- Mutual：必须配置 caSecretRef，可选 verifyDepth/allowedSans/allowedCns
- OptionalMutual：同上但验证可选

### 3.2 Secret

```
src/types/secret/index.ts
src/utils/secret.ts
src/components/ResourceEditor/Secret/
  ├── SecretEditor.tsx
  └── SecretForm.tsx                    # type 选择 + 文件上传/PEM 粘贴
src/pages/Security/SecretList.tsx
```

安全处理：
- tls.key 不明文展示
- Base64 编解码在前端处理
- 创建时支持文件上传和文本粘贴

### 3.3 BackendTLSPolicy

SimpleResourceEditor + 基础列表页。

```
src/pages/Security/BackendTLSPolicyList.tsx
```

---

## Phase 4 — 插件扩展

### 4.1 EdgionStreamPlugins

```
src/types/edgion-stream-plugins/index.ts
src/utils/edgionstreamplugins.ts
src/components/ResourceEditor/EdgionStreamPlugins/
  ├── EdgionStreamPluginsEditor.tsx
  └── EdgionStreamPluginsForm.tsx       # plugins 列表：type + config
src/pages/Plugins/EdgionStreamPluginsList.tsx
```

当前只有 IpRestriction 插件，config 编辑器：
- ipSource: remoteAddr
- allow: CIDR 列表
- deny: CIDR 列表（优先级高于 allow）
- defaultAction: allow | deny
- message: 自定义拒绝消息

### 4.2 PluginMetaData

集群级资源，SimpleResourceEditor + 列表页。

```
src/pages/Plugins/PluginMetaDataList.tsx
```

### 4.3 LinkSys

```
src/types/link-sys/index.ts
src/utils/linksys.ts
src/components/ResourceEditor/LinkSys/
  ├── LinkSysEditor.tsx
  ├── LinkSysForm.tsx
  └── sections/                         # 按 type 条件渲染
      ├── RedisConfigSection.tsx
      ├── ElasticsearchConfigSection.tsx
      ├── EtcdConfigSection.tsx
      └── WebhookConfigSection.tsx
src/pages/System/LinkSysList.tsx
```

type 字段决定表单内容。密码字段用密码输入框。

### 4.4 侧边栏更新

- 插件组下添加 Stream Plugins
- 新增"系统"菜单组（或"外部集成"）包含 LinkSys

---

## Phase 5 — 系统配置

### 5.1 EdgionGatewayConfig

```
src/types/edgion-gateway-config/index.ts
src/utils/edgiongatewayconfig.ts
src/components/ResourceEditor/EdgionGatewayConfig/
  ├── EdgionGatewayConfigEditor.tsx
  ├── EdgionGatewayConfigForm.tsx
  └── sections/
      ├── ServerSection.tsx             # threads, workStealing, grace, keepalive, compression
      ├── HttpTimeoutSection.tsx        # client(read/write/keepalive) + backend(connect/request/idle)
      ├── RealIpSection.tsx             # trustedIps + header + recursive
      ├── SecuritySection.tsx           # XFF limit, SNI match, fallback, TLS log
      ├── GlobalPluginsSection.tsx      # 全局插件引用列表
      └── PreflightSection.tsx          # mode + statusCode
src/pages/System/EdgionGatewayConfigPage.tsx  # 可能是单例页面
```

apiVersion: `edgion.io/v1alpha1`（注意不是 v1）
通常只有一个实例，考虑单例编辑页而非列表。

### 5.2 EdgionAcme

```
src/types/edgion-acme/index.ts
src/utils/edgionacme.ts
src/components/ResourceEditor/EdgionAcme/
  ├── EdgionAcmeEditor.tsx
  ├── EdgionAcmeForm.tsx
  └── sections/
      ├── ChallengeSection.tsx          # http-01/dns-01 条件渲染
      ├── StorageSection.tsx            # secretName + namespace
      ├── RenewalSection.tsx            # renewBeforeDays + intervals
      └── AutoEdgionTlsSection.tsx      # 自动创建 EdgionTls 开关
src/pages/System/EdgionAcmeList.tsx
```

特殊功能：
- challenge.type 条件渲染（http-01: gatewayRef, dns-01: provider + credentialRef）
- status 只读展示（phase 状态徽章、证书到期时间、失败原因）
- 手动触发签发按钮（`POST /api/v1/services/acme/{ns}/{name}/trigger`）
- DNS 凭据安全处理

---

## Phase 6 — Dashboard 增强 & 体验优化

### 6.1 Dashboard 实时数据

- 调用 `/api/v1/namespaced/{kind}` count 获取各资源计数
- 调用 `/health` 和 `/ready` 展示健康状态
- 调用 `/api/v1/server-info` 展示系统信息（server_id, ready）

### 6.2 资源关系展示

- Gateway → Listener → Route 绑定关系
- Route → BackendRef → Service → EndpointSlice 链路
- EdgionTls → Secret 证书引用
- EdgionAcme → EdgionTls 自动创建关系

### 6.3 体验优化

- 面包屑导航
- 加载骨架屏（Ant Design Skeleton）
- 资源快速跳转（Route → Gateway, EdgionTls → Secret）
- 批量导入/导出 YAML
- 资源状态实时刷新

---

## 侧边栏菜单规划

```
Dashboard
路由管理
  ├── HTTPRoute       ✅
  ├── GRPCRoute
  ├── TCPRoute
  ├── UDPRoute
  └── TLSRoute
基础设施
  ├── Gateway
  ├── GatewayClass
  ├── Service
  ├── EndpointSlice
  └── ReferenceGrant
安全配置
  ├── EdgionTls
  ├── Secret
  └── BackendTLSPolicy
插件管理
  ├── EdgionPlugins   ✅
  ├── StreamPlugins
  └── PluginMetaData
系统配置
  ├── EdgionGatewayConfig
  ├── LinkSys
  └── EdgionAcme
```

## 执行策略

### 开发顺序

Phase 0 必须最先做（后续所有 Phase 依赖）。之后推荐：

1. **Phase 0** → **Phase 1.1 (TCP/UDP)** 验证公共组件
2. **Phase 1.2 (TLS)** → **Phase 1.3 (GRPC)**
3. **Phase 2 (Gateway 最复杂)** → Phase 2 其余
4. Phase 3-5 顺序灵活，按实际需求调整
5. Phase 6 可以分散到各 Phase 中

### 每个资源的开发步骤

1. 读后端用户文档：`edgion/docs/zh-CN/` 中对应文件
2. 读后端 Skills Schema：`edgion/skills/02-features/03-resources/`
3. 定义 TypeScript 类型 → 实现工具函数 → 开发编辑器 → 开发列表页 → 注册路由
4. 启动后端（`start_all_with_conf.sh`）+ 加载数据 + 浏览器验证

### 测试数据

```bash
cd /Users/caohao/ws2/edgion
./examples/test/scripts/utils/start_all_with_conf.sh
./examples/test/scripts/utils/load_conf.sh all
```

---

## 工作量估算

| Phase | 新代码行数 | 资源数 | 核心复杂度 |
|-------|-----------|--------|-----------|
| Phase 0 | ~400 | — | 重构+提取+API更新 |
| Phase 1 | ~1,400 | 4 | GRPCRoute 匹配编辑器 |
| Phase 2 | ~1,500 | 5 | Gateway Listener 编辑器 |
| Phase 3 | ~1,300 | 3 | EdgionTls mTLS + Secret 安全 |
| Phase 4 | ~1,200 | 3 | LinkSys 条件渲染 |
| Phase 5 | ~1,300 | 2 | GatewayConfig 多区段 + ACME 状态 |
| Phase 6 | ~800 | — | API 集成 + 关系展示 |
| **总计** | **~7,900** | **18** | — |

当前代码 4,749 行 → 完成后约 12,600 行。
