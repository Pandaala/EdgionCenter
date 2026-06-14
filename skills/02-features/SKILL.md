---
name: center-features
description: How to run and configure EdgionCenter — config schema, ports, deployment, auth bootstrap, Admin API endpoints.
---

# 02 Features

Operator-facing reference for EdgionCenter. Authoritative config example:
[`config/edgion-center.yaml`](../../config/edgion-center.yaml).

## Ports

| Port | Listener | Serves |
|------|----------|--------|
| `12251` | `server.grpc_addr` | Federation gRPC (Controller → Center) |
| `12201` | `server.http_addr` | Admin HTTP API |
| `12200` | `server.probe_addr` | `/health`, `/ready` only |
| `12290` | `server.metrics_addr` | Prometheus metrics |

## Sync configuration (`sync:`)

| Key | Default | Meaning |
|-----|---------|---------|
| `list_interval_secs` | 300 | ListRequest cadence per controller |
| `list_timeout_secs` | 30 | ListResponse wait timeout |
| `command_timeout_secs` | 30 | CommandResponse wait timeout |
| `ping_interval_secs` | 30 | Heartbeat ping interval |

## Authentication

Mandatory. Configure a `local_auth` or `auth` (OIDC discovery) provider; omitting both leaves
business routes returning 503 until configured. `/health`, `/ready`, `/metrics`,
`/api/v1/auth/status` remain reachable. See
[01-architecture/06-center/SKILL.md](../01-architecture/06-center/SKILL.md) §Authentication.

## Access control (lite / full)

Two config-selected access-control tiers (`access.mode`): **lite** (login = admin) and
**full** (DB-backed users + page/API RBAC). Covers the `access:` / `database:` / `audit:`
config, the permission catalog, RBAC enforcement, admin bootstrap, and known limitations.
See [access-control.md](access-control.md).

## Admin API

TODO: enumerate Admin API endpoints. Handlers live in `src/api/`
(`consistency_handlers.rs`, `region_route_handlers.rs`,
`global_connection_ip_restriction_handlers.rs`, `web.rs`).

## Deployment

TODO: document deployment shapes. See `deploy/` and `docker/` in the repo root, and the
`embed-dashboard` feature in `Cargo.toml`.

## External dependency — Edgion features

Resource feature Schema (Route/Plugin/TLS/Backend/…) is defined upstream and not duplicated:
https://github.com/Pandaala/Edgion/tree/main/skills/02-features/03-resources
