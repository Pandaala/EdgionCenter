---
name: system-resources
description: 系统配置资源开发指南——EdgionGatewayConfig/LinkSys/EdgionAcme（基于 feature-04-06 用户文档）
---

# 系统配置资源

## EdgionGatewayConfig（待开发）

```yaml
apiVersion: edgion.io/v1alpha1
kind: EdgionGatewayConfig
metadata:
  name: default-config
spec:
  # Pingora 服务器配置
  server:
    threads: 0                              # uint32, 默认 CPU 核数
    workStealing: true                      # bool
    gracePeriodSeconds: 30                  # uint64
    gracefulShutdownTimeoutS: 10            # uint64
    upstreamKeepalivePoolSize: 128          # uint32
    enableCompression: false                # bool, 下游响应压缩
    downstreamKeepaliveRequestLimit: 1000   # uint32, 0=无限

  # HTTP 超时配置
  httpTimeout:
    client:
      readTimeout: "60s"
      writeTimeout: "60s"
      keepaliveTimeout: "75s"
    backend:
      defaultConnectTimeout: "5s"
      defaultRequestTimeout: "60s"
      defaultIdleTimeout: "300s"

  # 最大重试次数（从 annotation 迁移到此处）
  maxRetries: 3                             # uint32

  # Real IP 提取
  realIp:
    trustedIps: []                          # 可信代理 IP/CIDR
    realIpHeader: "X-Forwarded-For"         # 提取 Real IP 的 Header
    recursive: true                         # 从右向左遍历，跳过 trustedIps

  # 安全保护
  securityProtect:
    xForwardedForLimit: 200                 # XFF 最大字节
    requireSniHostMatch: true               # HTTPS 421 Misdirected Request 检测
    fallbackSni: ""                         # 客户端无 SNI 时的兜底
    tlsProxyLogRecord: true                 # 记录 TLS 代理连接日志

  # 全局插件引用
  globalPluginsRef:                         # 应用到所有路由的全局插件
    - name: "global-cors"
      namespace: "edgion-system"

  # 预检策略
  preflightPolicy:
    mode: "cors-standard"                   # "cors-standard" | "all-options"
    statusCode: 204                         # 无 CORS 插件时的响应码

  # ReferenceGrant 验证
  enableReferenceGrantValidation: false     # bool
```

**开发要点**：
- **集群级资源**，用 `clusterResourceApi`，kind: `edgiongatewayconfig`
- apiVersion: `edgion.io/v1alpha1`（注意不是 v1）
- 通过 GatewayClass.spec.parametersRef 关联
- 通常只有一个实例（考虑单例编辑页）
- 表单区段（按功能分组）：
  - **Server** — threads, workStealing, gracePeriod, keepalive, compression
  - **HTTP Timeout** — client(read/write/keepalive) + backend(connect/request/idle)
  - **Max Retries** — 全局上游最大重试
  - **Real IP** — trustedIps 列表 + header + recursive
  - **Security** — XFF limit, SNI/Host 匹配, fallback SNI, TLS 日志
  - **Global Plugins** — 全局插件引用列表
  - **Preflight** — mode 选择 + statusCode
  - **ReferenceGrant** — 开关

## LinkSys（待开发）

```yaml
apiVersion: edgion.io/v1
kind: LinkSys
metadata:
  name: redis-cluster
  namespace: default
spec:
  type: redis                   # redis | elasticsearch | etcd | webhook
  redis:
    addresses:
      - "127.0.0.1:6379"
    password: "secret"
    database: 0
    clusterMode: false
    tls:
      enable: false
```

**开发要点**：
- 命名空间资源，kind: `linksys`
- type 决定 spec 具体结构（条件渲染）
- 四种类型：redis, elasticsearch, etcd, webhook
- **安全敏感**：password 字段用密码输入框
- 表单按 type 切换不同配置区段
- 列表页展示：name, namespace, type, 连接地址

## EdgionAcme（待开发）

```yaml
apiVersion: edgion.io/v1
kind: EdgionAcme
metadata:
  name: lets-encrypt
  namespace: default
spec:
  email: "admin@example.com"                    # 必填：ACME 账户邮箱
  domains:                                       # 必填：证书域名
    - "example.com"
    - "*.example.com"                            # DNS-01 支持通配符
  server: "https://acme-v02.api.letsencrypt.org/directory"  # 可选
  keyType: "ecdsa-p256"                          # 可选：ecdsa-p256(默认) | ecdsa-p384

  challenge:                                     # 必填
    type: http-01                                # http-01 | dns-01
    http01:
      gatewayRef:                                # http-01 必填
        name: my-gateway
        namespace: default
    dns01:                                       # dns-01 必填
      provider: cloudflare                       # cloudflare | alidns
      credentialRef:                             # DNS API 凭据 Secret
        name: cloudflare-api-token
        namespace: default
      propagationTimeout: 120                    # DNS 传播超时（秒）
      propagationCheckInterval: 5                # DNS 检查间隔（秒）

  storage:                                       # 必填：证书存储
    secretName: "acme-cert"
    secretNamespace: default

  renewal:                                       # 可选：续期配置
    renewBeforeDays: 30                          # 到期前多少天续期
    checkInterval: 86400                         # 检查间隔（秒）
    failBackoff: 300                             # 失败重试延迟（秒）

  autoEdgionTls:                                 # 可选：自动创建 EdgionTls
    enabled: true
    name: "acme-lets-encrypt"                    # EdgionTls 名称
    parentRefs:                                  # 绑定 Gateway
      - name: my-gateway

status:                                          # 只读
  phase: Ready                                   # Pending|Issuing|Ready|Renewing|Failed
  certificateSerial: "xxx"
  certificateNotAfter: "2026-07-10T00:00:00Z"
  lastFailureReason: ""
  secretName: "acme-cert"
  edgionTlsName: "acme-lets-encrypt"
```

**开发要点**：
- 命名空间资源，kind 需添加到 ResourceKind: `edgionacme`
- 表单区段：
  - 基本信息（email, server, keyType）
  - 域名列表编辑
  - Challenge 配置（http-01/dns-01 条件渲染）
    - http-01: gatewayRef 选择
    - dns-01: provider + credentialRef + propagation 配置
  - Storage 配置
  - Renewal 配置
  - AutoEdgionTls 配置（开关 + 名称 + parentRefs）
- Status 只读展示：phase 状态徽章、证书到期时间、失败原因
- **安全敏感**：DNS API 凭据
- 支持手动触发签发：`POST /api/v1/services/acme/{namespace}/{name}/trigger`
- 列表页展示：name, namespace, phase(Tag), domains, challenge type, 到期时间
- 需要在侧边栏添加菜单项
