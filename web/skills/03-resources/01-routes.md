---
name: route-resources
description: 路由类资源开发指南——HTTPRoute/GRPCRoute/TCPRoute/UDPRoute/TLSRoute 的完整 Schema（基于 feature-04-06 用户文档）
---

# 路由类资源

## 共同特征

所有路由资源共享：
- `spec.parentRefs` — 绑定 Gateway/Listener
- `spec.rules` — 路由规则
- 命名空间资源，用 `resourceApi`

## HTTPRoute ✅ 已完成

- apiVersion: `gateway.networking.k8s.io/v1`
- Kind: `httproute`
- 参考代码：`src/pages/Routes/HTTPRouteList.tsx`、`src/components/ResourceEditor/HTTPRoute/`

完整 Schema 见后端文档：`edgion/skills/02-features/03-resources/04-httproute.md`

关键字段：
- `spec.parentRefs` — Gateway 绑定
- `spec.hostnames` — 主机名匹配（支持通配符）
- `spec.rules[].matches` — 匹配条件（path/headers/queryParams/method，OR 关系）
- `spec.rules[].filters` — 过滤器链（RequestHeaderModifier, ResponseHeaderModifier, RequestRedirect, URLRewrite, RequestMirror, ExtensionRef）
- `spec.rules[].backendRefs` — 后端引用（name/port/weight，支持 backendRef 级 filter）
- `spec.rules[].timeouts` — request/backendRequest 超时
- `spec.rules[].retry` — attempts/backoff/codes 重试策略
- `spec.rules[].sessionPersistence` — 会话亲和（Cookie/Header）

**Edgion 扩展字段**：
- `extensionRefMaxDepth` — ExtensionRef 嵌套深度限制
- `sessionPersistence.strict` — 严格亲和模式
- RequestMirror 扩展：`connectTimeoutMs`, `writeTimeoutMs`, `maxBufferedChunks`, `mirrorLog`, `maxConcurrent`

## GRPCRoute（待开发）

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: GRPCRoute
metadata:
  name: my-grpc-route
  namespace: default
spec:
  parentRefs:
    - name: my-gateway
      sectionName: grpc-https
  hostnames:
    - "grpc.example.com"
  rules:
    - matches:
        - method:
            type: Exact                # Exact | RegularExpression
            service: "mypackage.MyService"  # gRPC service FQDN
            method: "GetItem"               # gRPC method name
          headers:
            - type: Exact
              name: x-custom-header
              value: "value"
      filters:
        - type: RequestHeaderModifier
          requestHeaderModifier:
            set: [{ name: x-backend-version, value: "v2" }]
        - type: ResponseHeaderModifier
          responseHeaderModifier:
            add: [{ name: x-trace-id, value: "{{generated}}" }]
        - type: RequestMirror
          requestMirror:
            backendRef: { name: mirror-service, port: 50051 }
            fraction: { numerator: 5, denominator: 100 }
        - type: ExtensionRef
          extensionRef: { group: edgion.io, kind: EdgionPlugins, name: grpc-auth }
      backendRefs:
        - name: grpc-service
          port: 50051
          weight: 100
      timeouts:
        request: "30s"
        backendRequest: "10s"
      retry:
        attempts: 3
        backoff: "500ms"
        codes: [14]  # gRPC status codes（parsed but runtime-ignored）
      sessionPersistence:
        type: Cookie
        sessionName: "GRPC_SESSION"
```

**开发要点**：
- 与 HTTPRoute 非常相似，主要区别在 matches 用 `method`（gRPC service/method）替代 `path`
- **不支持** RequestRedirect 和 URLRewrite 过滤器
- `retry.codes` 是 gRPC status codes（0-16），parsed but **runtime-ignored**
- 自动检测和支持 gRPC-Web 请求
- 可以大量复用 HTTPRoute 的组件

**GRPCMethodMatch 结构**：
| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | string | 否 | Exact（默认）\| RegularExpression |
| `service` | string | 是 | gRPC 服务全名（如 `billing.v1.BillingService`） |
| `method` | string | 是 | gRPC 方法名（如 `CreateInvoice`） |

## TCPRoute（待开发）

```yaml
apiVersion: gateway.networking.k8s.io/v1alpha2
kind: TCPRoute
metadata:
  name: my-tcp-route
  namespace: default
  annotations:
    edgion.io/edgion-stream-plugins: "default/my-stream-plugins"  # StreamPlugins 绑定
    edgion.io/proxy-protocol: "1"       # Proxy Protocol 版本（1|2）
    edgion.io/max-connect-retries: "3"  # 最大连接重试次数
spec:
  parentRefs:
    - name: my-gateway
      sectionName: tcp-9000
  rules:
    - backendRefs:
        - name: tcp-service
          port: 9000
          weight: 100
```

**开发要点**：
- apiVersion: `gateway.networking.k8s.io/v1alpha2`
- 最简单的路由类型：**无 matches、无 hostnames、无 filters**
- 只有 `parentRefs` + `rules[].backendRefs`
- 通过 annotations 绑定 StreamPlugins、Proxy Protocol、连接重试
- 表单需要 annotations 编辑支持
- 适用场景：Redis, MySQL, PostgreSQL, MQTT 等 TCP 协议

## UDPRoute（待开发）

```yaml
apiVersion: gateway.networking.k8s.io/v1alpha2
kind: UDPRoute
metadata:
  name: my-udp-route
  namespace: default
spec:
  parentRefs:
    - name: my-gateway
      sectionName: udp-5300
  rules:
    - backendRefs:
        - name: udp-service
          port: 5300
```

**开发要点**：
- 与 TCPRoute 结构几乎一致
- 适用场景：DNS, 日志采集, 游戏通信等无状态协议
- UDP 无连接，不支持 StreamPlugins annotations
- 可以与 TCPRoute 共享编辑器组件

## TLSRoute（待开发）

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: TLSRoute
metadata:
  name: my-tls-route
  namespace: default
  annotations:
    edgion.io/edgion-stream-plugins: "default/my-stream-plugins"
    edgion.io/proxy-protocol: "2"
    edgion.io/max-connect-retries: "3"
spec:
  parentRefs:
    - name: my-gateway
      sectionName: tls-passthrough
  hostnames:           # SNI 匹配
    - "secure.example.com"
    - "*.internal.example.com"
  rules:
    - backendRefs:
        - name: tls-backend
          port: 8443
          weight: 100
```

**开发要点**：
- apiVersion: `gateway.networking.k8s.io/v1`（从 v1alpha3 晋升到 v1）
- 基于 TLS ClientHello 中的 SNI 进行路由
- 比 TCPRoute 多了 `hostnames`（SNI 匹配）
- 支持 StreamPlugins、Proxy Protocol annotations
- 无 matches、无 filters

## 路由资源复用矩阵

| 组件 | HTTPRoute | GRPCRoute | TCPRoute | UDPRoute | TLSRoute |
|------|-----------|-----------|----------|----------|----------|
| MetadataSection | ✅ | 复用 | 复用 | 复用 | 复用 |
| AnnotationsEditor | — | — | 新建(stream) | — | 复用(stream) |
| ParentRefsSection | ✅ | 复用 | 复用 | 复用 | 复用 |
| HostnamesSection | ✅ | 复用 | ❌ | ❌ | 复用 |
| PathMatchField | ✅ | ❌ | ❌ | ❌ | ❌ |
| HeaderMatchField | ✅ | 复用 | ❌ | ❌ | ❌ |
| GRPCMethodMatch | ❌ | 新建 | ❌ | ❌ | ❌ |
| BackendRefsEditor | ✅ | 复用 | 复用 | 复用 | 复用 |
| FiltersEditor | ✅ | 复用(部分) | ❌ | ❌ | ❌ |
| TimeoutsEditor | ✅ | 复用 | ❌ | ❌ | ❌ |
| RetryEditor | ✅ | 复用 | ❌ | ❌ | ❌ |
| SessionPersistence | ✅ | 复用 | ❌ | ❌ | ❌ |

**结论**：提取公共组件到 `src/components/ResourceEditor/common/`。
