---
name: dashboard-resources
description: 各种 K8s/Edgion 资源的前端开发指南——Schema、字段映射、特殊处理
---

# 资源开发指南

每种资源的 Schema 来自 Edgion 后端项目：`edgion/skills/02-features/03-resources/`。
开发前先读对应的后端 Schema 文件，了解完整字段定义。

## 资源分类

| 文件 | 资源 | 后端 Schema 参考 |
|------|------|-----------------|
| [01-routes.md](01-routes.md) | HTTPRoute ✅, GRPCRoute, TCPRoute, UDPRoute, TLSRoute | `edgion/skills/02-features/03-resources/01-routes/` |
| [02-infrastructure.md](02-infrastructure.md) | Gateway, GatewayClass, Service, EndpointSlice | `edgion/skills/02-features/03-resources/02-infrastructure/` |
| [03-security.md](03-security.md) | EdgionTls, Secret, BackendTLSPolicy | `edgion/skills/02-features/03-resources/03-tls/` |
| [04-plugins.md](04-plugins.md) | EdgionPlugins ✅, EdgionStreamPlugins, PluginMetaData | `edgion/skills/02-features/03-resources/04-plugins/` |
| [05-system.md](05-system.md) | EdgionGatewayConfig, LinkSys, EdgionAcme | `edgion/skills/02-features/03-resources/05-system/` |

## 通用字段

所有资源共享的字段（来自 `K8sResource`）：

```yaml
apiVersion: <group>/<version>     # e.g., gateway.networking.k8s.io/v1
kind: <ResourceName>              # e.g., HTTPRoute
metadata:
  name: <dns-1123-subdomain>      # 必填
  namespace: <dns-1123-label>     # 命名空间资源必填，集群资源无此字段
  labels: {}                      # 可选
  annotations: {}                 # 可选
spec: {}                          # 资源特有
status: {}                        # 只读，后端维护
```

## 资源复杂度分级

| 级别 | 资源 | 说明 |
|------|------|------|
| 简单（YAML-only） | Service, EndpointSlice, GatewayClass, PluginMetaData, BackendTLSPolicy | 只读展示 + YAML 编辑器，不需要复杂表单 |
| 中等（基础表单） | TCPRoute, UDPRoute, TLSRoute, Gateway, EdgionTls, Secret, LinkSys | Metadata + 少量 spec 字段的表单 |
| 复杂（完整表单） | HTTPRoute ✅, GRPCRoute, EdgionPlugins ✅, EdgionGatewayConfig, EdgionAcme | 嵌套表单 + 多区段 + 条件渲染 |

### 简单资源的简化编辑器

对于只读/YAML-only 资源，可以使用简化的编辑器模式：
- 不需要 Form 标签页，只用 YAML 编辑器
- 列表页展示关键字段
- 创建/编辑时直接编辑 YAML
