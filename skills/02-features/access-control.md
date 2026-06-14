---
name: center-access-control
description: Dual-tier access control for EdgionCenter — lite vs full, the [access]/[database]/[audit] config, the permission catalog, RBAC enforcement, admin bootstrap, the audit log, and known limitations.
---

# Access control (lite / full)

Center ships two config-selected access-control tiers, chosen by `access.mode`.
Authoritative config example: [`config/edgion-center.yaml`](../../config/edgion-center.yaml).
Code of record: `src/config/mod.rs` (`AccessConfig` / `AccessMode` / `DatabaseConfig` /
`AuditConfig`), `src/cli/mod.rs` (tier wiring + bootstrap), `src/common/authz/catalog.rs`
(permission catalog + route map), `src/common/authz/db_authz.rs` (full-tier authz).

## lite vs full at a glance

| | **lite** (default) | **full** |
|---|---|---|
| Authentication | OIDC/Okta (`auth:`) and/or single shared admin (`local_auth:`) | DB-backed local users (username + bcrypt password) |
| Authorization | Login = admin: every authenticated caller is a full admin | Page + API RBAC via permission keys (roles bundle keys, users get roles) |
| User/role admin | Hidden (dashboard hides Users/Roles) | Managed from the dashboard Users + Roles pages |
| Storage | SQLite (default) | SQLite (default); MySQL recommended |
| Database required | No (audit needs it; otherwise optional) | Yes — startup fails without a usable DB |
| OIDC (`auth:`) | Honored for SSO login | Ignored (WARN logged if configured) |
| `local_auth:` | Single-admin login + JWT secret | Only `jwt_secret` used (signer); username/password ignored |
| Audit log | User-attributed | User-attributed |

Both tiers share the same audit log and the same fail-closed authn (no `auth:` and no
`local_auth:` → business routes return 503; `/health`, `/ready`, `/metrics`,
`/api/v1/auth/status` stay reachable).

## Config keys

### `access:`
| Key | Default | Meaning |
|-----|---------|---------|
| `mode` | `lite` | Access-control tier: `lite` or `full`. |

### `database:`
| Key | Default | Meaning |
|-----|---------|---------|
| `enabled` | `true` | When false, Center runs without persistence (full mode then fails to start). |
| `backend` | `sqlite` | `sqlite` (embedded) or `mysql` (external). |
| `sqlite_path` | `data/center.db` | SQLite file path (when `backend = sqlite`). |
| `mysql_url` | `null` | MySQL URL `mysql://user:pass@host:3306/db` — REQUIRED when `backend = mysql`; ignored otherwise. |

Full mode REQUIRES a usable database; Center refuses to start if `database.enabled = false`
or the backend is unreachable.

### `audit:`
| Key | Default | Meaning |
|-----|---------|---------|
| `enabled` | `true` | Requires `database.enabled = true`; with no DB, auditing is skipped (WARN at startup). |
| `log_reads` | `false` | Record GET reads too. Mutations are always recorded when enabled. |
| `retention_days` | `0` | Retention window; `0` = keep indefinitely. **Not enforced yet** (see Known limitations). |

The dashboard Audit page reads `GET /api/v1/center/admin/audit-logs` (gated by `audit:read`).

### `auth:` / `local_auth:` interaction per mode
- **lite** — `auth:` (OIDC discovery) and/or `local_auth:` (single shared admin) provide login.
  Either or both may be configured; both can be enabled simultaneously.
- **full** — `auth:` is ignored (DB users only; a WARN is logged if it is set). `local_auth:`
  is consulted ONLY for `jwt_secret` (the HS256 secret that signs/validates DB-user login
  tokens); its `username`/`password` are ignored. A `local_auth:` block with a non-empty
  `jwt_secret` is REQUIRED — startup fails without both a usable DB and a `jwt_secret`.
  The signing secret comes solely from the `local_auth.jwt_secret` YAML field (the same
  field lite uses to sign/validate local tokens); there is no environment override for it.

## Permission catalog

Source of truth: `src/common/authz/catalog.rs` (`catalog_groups()` / `all_keys()`). Eleven
keys in seven groups. Each business API route + dashboard page maps to exactly one key; roles
bundle keys; users get roles.

| Group | Keys |
|-------|------|
| Controllers | `controllers:read`, `controllers:write` |
| Region Routes | `region-routes:read`, `region-routes:write` |
| IP Restrictions | `ip-restrictions:read`, `ip-restrictions:write` |
| Audit | `audit:read` |
| Server | `server:read` |
| Proxy | `proxy:access` |
| Access Control | `users:manage`, `roles:manage` |

GET endpoints map to a `:read` key, mutating endpoints to a `:write` key.
`users:manage` gates `/api/v1/center/admin/users`; `roles:manage` gates
`/api/v1/center/admin/roles` and `/api/v1/center/admin/permission-catalog`.

## How RBAC enforcement works (full tier)

1. `route_permission(method, path)` in `catalog.rs` maps the concrete request to its required
   key (the single source of truth consulted by the authz middleware).
2. The authz middleware looks up the caller's keys (`DbAuthz`, backed by the `users`/`roles`
   tables) and returns **403** if the required key is absent.
3. **Deny-by-default**: `is_business_path()` marks every `/api/v1/` path except `/api/v1/auth/*`
   as a business path. An unmapped business route denies by default for non-superusers, so a
   newly added route never leaks access before it is added to the catalog.
4. `/auth/me` returns the caller's effective keys; the dashboard gates menus by them and hides
   the Users/Roles pages entirely in lite (driven by `accessMode` from `/api/v1/server-info`).

In **lite** mode the installed store is `AllowAllAuthz`: every authenticated caller reports the
full key set, so all menus/routes are available (login = admin).

## First-run admin bootstrap (full mode)

When `access.mode: full` and the `users` table is empty, set BOTH env vars before start:

```
EDGION_ADMIN_USERNAME=admin
EDGION_ADMIN_PASSWORD=<strong-password>
```

Center creates an `admin` user (bcrypt-hashed password) bound to a built-in `admin` role that
holds every permission key. If either var is unset, a WARN is logged and no login is possible
until a user is provisioned. After bootstrap, manage users/roles from the dashboard.

## Audit log

Records who/what/when/from-where/result for mutating admin actions (and reads when
`log_reads = true`). Requires `database.enabled = true`. Attribution works in both tiers
(the audit log is the compensating control in lite, where everyone is an admin). Browse it
from the dashboard Audit page (`audit:read`).

## Known limitations

- **`retention_days` is not enforced**: pruning is not yet scheduled, so audit records are
  never auto-deleted regardless of the configured value.
- **~30s permission staleness**: `DbAuthz` caches per-subject key sets with a 30s TTL
  (`CACHE_TTL` in `db_authz.rs`), so role/permission changes take up to ~30s to take effect.
- **Single-Center only**: the DB-backed RBAC tier assumes one Center (no cross-Center sync of
  users/roles/permissions).

## Deeper detail

- Design: [`docs/history/superpowers/specs/2026-06-13-center-dual-access-control-design.md`](../../docs/history/superpowers/specs/2026-06-13-center-dual-access-control-design.md)
- Plan: [`docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md`](../../docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md)
