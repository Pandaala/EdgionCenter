---
name: dashboard-resources
description: Frontend development guide for K8s/Edgion resources — Schema, field mapping, and special handling
---

> **Resource Schema is authoritative upstream (Edgion), not here.** This tree keeps
> frontend page-development notes only. Field definitions, types, and defaults:
> https://github.com/Pandaala/Edgion/tree/main/skills/02-features/03-resources

# Resource Development Guide

Each resource's Schema originates from the Edgion backend project: `edgion/skills/02-features/03-resources/`.
Read the corresponding backend Schema file before development to understand the complete field definitions.

## Resource Categories

| File | Resource | Backend Schema Reference |
|------|------|-----------------|
| [01-routes.md](01-routes.md) | HTTPRoute ✅, GRPCRoute, TCPRoute, UDPRoute, TLSRoute | `edgion/skills/02-features/03-resources/01-routes/` |
| [02-infrastructure.md](02-infrastructure.md) | Gateway, GatewayClass, Service, EndpointSlice | `edgion/skills/02-features/03-resources/02-infrastructure/` |
| [03-security.md](03-security.md) | EdgionTls, Secret, BackendTLSPolicy | `edgion/skills/02-features/03-resources/03-tls/` |
| [04-plugins.md](04-plugins.md) | EdgionPlugins ✅, EdgionStreamPlugins, PluginMetaData | `edgion/skills/02-features/03-resources/04-plugins/` |
| [05-system.md](05-system.md) | EdgionGatewayConfig, LinkSys, EdgionAcme | `edgion/skills/02-features/03-resources/05-system/` |

## Common Fields

Fields shared by all resources (from `K8sResource`):

```yaml
apiVersion: <group>/<version>     # e.g., gateway.networking.k8s.io/v1
kind: <ResourceName>              # e.g., HTTPRoute
metadata:
  name: <dns-1123-subdomain>      # required
  namespace: <dns-1123-label>     # required for namespaced resources, absent for cluster-scoped resources
  labels: {}                      # optional
  annotations: {}                 # optional
spec: {}                          # resource-specific
status: {}                        # read-only, managed by backend
```

## Resource Complexity Levels

| Level | Resource | Description |
|------|------|------|
| Simple (YAML-only) | Service, EndpointSlice, GatewayClass, PluginMetaData, BackendTLSPolicy | Read-only display + YAML editor; no complex form needed |
| Medium (basic form) | TCPRoute, UDPRoute, TLSRoute, Gateway, EdgionTls, Secret, LinkSys | Form with Metadata + a few spec fields |
| Complex (full form) | HTTPRoute ✅, GRPCRoute, EdgionPlugins ✅, EdgionGatewayConfig, EdgionAcme | Nested form + multiple sections + conditional rendering |

### Simplified Editor for Simple Resources

For read-only / YAML-only resources, a simplified editor mode can be used:
- No Form tab needed; use the YAML editor only
- List page displays key fields
- Create / edit by directly editing YAML
