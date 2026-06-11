---
name: infrastructure-resources
description: 基础设施资源开发指南——Gateway/GatewayClass/Service/EndpointSlice/ReferenceGrant（基于 feature-04-06 用户文档）
---

# 基础设施资源

## Gateway（待开发）

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: Gateway
metadata:
  name: my-gateway
  namespace: default
  annotations:
    edgion.io/enable-http2: "true"                   # HTTP/2 支持（默认 true）
    edgion.io/http-to-https-redirect: "true"         # HTTP→HTTPS 自动跳转
    edgion.io/https-redirect-port: "443"             # HTTPS 跳转端口
    edgion.io/edgion-stream-plugins: "ns/name"       # Gateway 级 StreamPlugins
spec:
  gatewayClassName: edgion                           # 必填：关联 GatewayClass
  listeners:
    - name: http
      port: 80
      protocol: HTTP                                 # HTTP | HTTPS | TCP | TLS | UDP
      hostname: "*.example.com"                      # 可选：主机名过滤
      allowedRoutes:
        namespaces:
          from: Same                                 # Same | All | Selector
        kinds:
          - group: gateway.networking.k8s.io
            kind: HTTPRoute

    - name: https
      port: 443
      protocol: HTTPS
      tls:                                           # HTTPS/TLS 协议必填
        mode: Terminate                              # Terminate | Passthrough
        certificateRefs:
          - name: my-cert-secret
            namespace: default                       # 跨命名空间需要 ReferenceGrant
        frontendValidation:                          # 可选：客户端证书验证
          caCertificateRefs:
            - name: client-ca
        options:
          edgion.io/cert-provider: "edgion-tls"      # "secret"(默认) | "edgion-tls"

    - name: tcp-redis
      port: 6379
      protocol: TCP

    - name: tls-passthrough
      hostname: "secure.example.com"
      port: 8443
      protocol: TLS
      tls:
        mode: Passthrough

    - name: udp-dns
      port: 5353
      protocol: UDP

  addresses:                                         # 可选
    - type: IPAddress
      value: "10.0.0.1"

status:                                              # 只读
  addresses: [...]
  conditions:
    - type: Accepted
      status: "True"
  listeners:
    - name: http
      attachedRoutes: 3
      conditions: [...]
```

**开发要点**：
- 命名空间资源，kind: `gateway`
- 核心是 `listeners` 数组管理
- protocol 枚举：HTTP, HTTPS, TCP, TLS, UDP
- TLS 配置仅在 HTTPS/TLS 时出现（条件渲染）
- annotations 控制 HTTP/2、HTTPS 跳转、StreamPlugins
- status 只读展示（listener 状态、attachedRoutes、地址）
- 列表页重点展示：name, namespace, listener 数量/端口, 绑定路由数

**表单区段**：
- MetadataSection + AnnotationsSection（HTTP/2, HTTPS redirect 开关）
- GatewayClassName 选择器
- ListenersSection（动态增删 listener）
  - ListenerEditor（name + protocol + port + hostname + TLS + allowedRoutes）

## GatewayClass（待开发）

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: GatewayClass
metadata:
  name: edgion
spec:
  controllerName: edgion.io/gateway-controller       # 必填
  parametersRef:                                      # 可选：关联 EdgionGatewayConfig
    group: edgion.io
    kind: EdgionGatewayConfig
    name: default-config
  description: "Edgion Gateway Controller"            # 可选
```

**开发要点**：
- **集群级资源**，用 `clusterResourceApi`，kind: `gatewayclass`
- 结构简单：controllerName + parametersRef + description
- 通常只有一个实例
- parametersRef 关联 EdgionGatewayConfig
- 列表页展示关联的 Gateway 数量
- YAML 编辑为主 + 基础信息展示

## Service（待开发 — 只读）

```yaml
apiVersion: v1
kind: Service
metadata:
  name: backend-service
  namespace: default
spec:
  type: ClusterIP
  selector:
    app: backend
  ports:
    - name: http
      port: 80
      targetPort: 8080
      protocol: TCP
```

**开发要点**：
- 命名空间资源，kind: `service`
- **只读展示**（Service 由 K8s 管理或用户通过 YAML 创建）
- 列表页展示：name, namespace, type, ports(Tags), selector
- 支持 YAML 查看和编辑
- 关联展示：引用该 Service 的 Route、关联的 EndpointSlice

## EndpointSlice（待开发 — 只读）

```yaml
apiVersion: discovery.k8s.io/v1
kind: EndpointSlice
metadata:
  name: backend-service-abc
  namespace: default
  labels:
    kubernetes.io/service-name: backend-service
addressType: IPv4
ports:
  - name: http
    port: 8080
    protocol: TCP
endpoints:
  - addresses: ["10.0.0.1"]
    conditions:
      ready: true
      serving: true
```

**开发要点**：
- 命名空间资源，kind: `endpointslice`
- **纯只读展示**
- 列表页展示：name, namespace, 关联 service, endpoint 数量, ready 状态
- 详情展示 endpoints 列表

## ReferenceGrant（待开发）

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: ReferenceGrant
metadata:
  name: allow-gateway-secret
  namespace: security          # 必须在目标资源的命名空间
spec:
  from:
    - group: gateway.networking.k8s.io
      kind: Gateway
      namespace: gateway-system     # 允许来源命名空间
  to:
    - group: ""                     # core/v1
      kind: Secret                  # 允许引用 Secret
```

**开发要点**：
- 命名空间资源，kind: `referencegrant`（需要添加到 ResourceKind）
- 控制跨命名空间资源引用权限
- 表单：from（group + kind + namespace 列表）+ to（group + kind 列表）
- 列表页展示：name, namespace, from 资源类型/命名空间, to 资源类型
- 需要在侧边栏添加菜单项
