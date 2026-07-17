---
name: center-architecture
description: Crate and binary map for EdgionCenter internals.
---

# Architecture

| Path | Responsibility |
|---|---|
| `crates/center-core/` | Platform-neutral ports, models, capabilities, audit, authz, coordination |
| `crates/center-runtime/` | Federation service, registry, aggregation, watches, command/proxy dispatch, internal forwarding |
| `crates/center-app/` | Shared Admin API, authentication, web assets, runtime re-exports |
| `crates/center-adapter-sql/` | SQLite/MySQL controller, user/RBAC, and audit persistence |
| `crates/center-adapter-kubernetes/` | Controller CRD projection, Lease fencing, owner lookup, SAR, stdout audit |
| `bins/edgion-center-standalone/` | Strict SQL-backed process composition and configuration |
| `bins/edgion-center-kubernetes/` | Strict database-free Kubernetes process composition and configuration |

The dependency direction is `core <- runtime/app/adapters <- binaries`. Adapters do not
depend on each other. Read [06-center/SKILL.md](06-center/SKILL.md) for the runtime model.

Independent cloud infrastructure expansion starts with
[07-cloud-integration.md](07-cloud-integration.md). Cloud resources are owned by Center and
do not extend Edgion resource schemas or federation contracts unless a later integration
explicitly requires Controller participation.
