---
name: security-resources
description: 安全资源开发指南——EdgionTls/Secret/BackendTLSPolicy（基于 feature-04-06 用户文档）
---

# 安全 & TLS 资源

## EdgionTls（待开发）

```yaml
apiVersion: edgion.io/v1
kind: EdgionTls
metadata:
  name: example-tls
  namespace: default
spec:
  parentRefs:                          # 可选：绑定 Gateway
    - name: my-gateway
      namespace: default
  hosts:                               # 必填：域名列表（支持通配符）
    - "*.example.com"
    - "api.example.com"
  secretRef:                           # 必填：服务端证书 Secret 引用
    name: example-cert
    namespace: default                 # 可选
  clientAuth:                          # 可选：mTLS 客户端认证
    mode: Mutual                       # Terminate(默认) | Mutual | OptionalMutual
    caSecretRef:                       # mode=Mutual/OptionalMutual 时必填
      name: client-ca
      namespace: default
    verifyDepth: 1                     # 证书链验证深度（1-9），默认 1
    allowedSans:                       # 可选：允许的客户端证书 SAN 白名单
      - "client1.example.com"
      - "*.internal.example.com"
    allowedCns:                        # 可选：允许的客户端证书 CN 白名单
      - "AdminClient"
  minTlsVersion: "TLS1_2"             # 可选：最低 TLS 版本 TLS1_0|TLS1_1|TLS1_2|TLS1_3
  cipherSuites:                        # 可选：自定义密码套件
    - ECDHE-RSA-AES256-GCM-SHA384
    - ECDHE-RSA-AES128-GCM-SHA256
    - ECDHE-RSA-CHACHA20-POLY1305
```

**开发要点**：
- 命名空间资源，kind: `edgiontls`
- 核心字段：hosts + secretRef + clientAuth + TLS 配置
- 表单区段：
  - MetadataSection
  - ParentRefsSection（绑定 Gateway，可选）
  - HostsSection — 域名列表编辑（支持通配符）
  - SecretRefSection — 证书引用选择器
  - ClientAuthSection — mTLS 配置（mode 条件渲染 caSecretRef 等）
  - TlsVersionSection — 最低版本下拉选择
  - CipherSuitesSection — 密码套件多选
- 列表页展示：name, namespace, hosts 数量, mTLS mode, TLS 版本

## Secret（待开发）

**TLS 类型**：
```yaml
apiVersion: v1
kind: Secret
metadata:
  name: my-cert
  namespace: default
type: kubernetes.io/tls
data:
  tls.crt: <base64>
  tls.key: <base64>
```

**CA 类型**：
```yaml
apiVersion: v1
kind: Secret
metadata:
  name: client-ca
  namespace: default
type: Opaque
data:
  ca.crt: <base64>
```

**开发要点**：
- 命名空间资源，kind: `secret`
- 类型枚举：`kubernetes.io/tls`, `Opaque`
- **安全敏感**：tls.key 不应在前端明文展示
- 创建表单：
  - type 选择（TLS 证书 / CA 证书 / 通用）
  - 文件上传（PEM 格式）或文本粘贴
  - Base64 编码在前端处理
- 列表页展示：name, namespace, type, data keys, 创建时间
- 查看模式：显示证书信息（有效期、CN 等），**隐藏**密钥内容
- 关联展示：引用该 Secret 的 EdgionTls/Gateway

## BackendTLSPolicy（待开发）

```yaml
apiVersion: gateway.networking.k8s.io/v1alpha3
kind: BackendTLSPolicy
metadata:
  name: backend-tls
  namespace: default
spec:
  targetRefs:
    - group: ""
      kind: Service
      name: backend-service
  validation:
    caCertificateRefs:
      - name: backend-ca
        group: ""
        kind: Secret
    hostname: "backend.internal"
    wellKnownCACertificates: ""        # 系统 CA（可选）
```

**开发要点**：
- 命名空间资源，kind: `backendtlspolicy`
- 定义网关 → 后端的 mTLS 策略
- 表单较简单：targetRef（Service 选择）+ validation（CA 引用 + hostname）
- YAML 编辑为主 + 基础表单
