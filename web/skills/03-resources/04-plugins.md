---
name: plugin-resources
description: 插件资源开发指南——EdgionPlugins/EdgionStreamPlugins/PluginMetaData（基于 feature-04-06 用户文档）
---

# 插件资源

## EdgionPlugins ✅ 已完成

- apiVersion: `edgion.io/v1`
- Kind: `edgionplugins`
- 参考代码：`src/pages/Plugins/EdgionPluginsList.tsx`

25+ 内置 HTTP 插件：
- **认证类**：Basic Auth, JWT Auth, Key Auth, HMAC Auth, LDAP Auth, Forward Auth, OpenID Connect, JWE Decrypt, Header Cert Auth
- **安全类**：CORS, CSRF, IP Restriction, Request Restriction
- **流量控制**：Rate Limit, Rate Limit(Redis), Proxy Rewrite, Response Rewrite, Bandwidth Limit, Request Mirror, Direct Endpoint, Dynamic Upstream, **Region Route（新增）**
- **可观测**：Real IP, Ctx Setter, Mock, DSL, Debug Access Log
- **Gateway API Filters**：Request Header Modifier, Response Header Modifier, Request Redirect, URL Rewrite

## EdgionStreamPlugins（待开发）

```yaml
apiVersion: edgion.io/v1
kind: EdgionStreamPlugins
metadata:
  name: my-stream-plugins
  namespace: default
spec:
  plugins:
    - type: IpRestriction
      config:
        ipSource: remoteAddr              # IP 来源：remoteAddr（连接 IP）
        allow:                            # IP 白名单（CIDR 格式）
          - "10.0.0.0/8"
          - "172.16.0.0/12"
        deny:                             # IP 黑名单（优先级高于 allow）
          - "10.0.0.100/32"
        defaultAction: allow              # 默认动作：allow | deny
        message: "Access denied"          # 拒绝时的消息
```

**IP 过滤逻辑**：deny 列表匹配 → 拒绝 → allow 列表匹配 → 允许 → defaultAction

**路由绑定方式**（通过 annotation）：
```yaml
# 同命名空间
annotations:
  edgion.io/edgion-stream-plugins: "my-stream-plugins"

# 跨命名空间
annotations:
  edgion.io/edgion-stream-plugins: "other-namespace/my-stream-plugins"
```

**支持协议**：Gateway listener 级连接过滤、TCPRoute、TLSRoute

**开发要点**：
- 命名空间资源，kind 需添加到 ResourceKind: `edgionstreamplugins`
- 比 EdgionPlugins 简单——**没有四阶段**，只有一个 plugins 列表
- 当前只有一个插件类型：IpRestriction
- 表单：metadata + plugins 列表编辑
  - type 选择（目前只有 IpRestriction）
  - config 编辑（ipSource, allow, deny, defaultAction, message）
- 列表页展示：name, namespace, 插件数量, 插件类型列表
- IP check 在连接建立时执行，性能影响小
- 插件配置支持热更新

## PluginMetaData（待开发）

```yaml
apiVersion: edgion.io/v1
kind: PluginMetaData
metadata:
  name: rate-limit
spec:
  description: "Rate limiting plugin"
  schema:
    type: object
    properties:
      count:
        type: integer
      time_window:
        type: integer
      key_type:
        type: string
        enum: ["var", "var_combination"]
  defaultConfig: {}
```

**开发要点**：
- **集群级资源**，用 `clusterResourceApi`，kind: `pluginmetadata`
- 插件元数据和 JSON Schema 定义
- YAML 编辑为主
- 列表页展示：name, description
- 列表页不需要 namespace 列
