---
name: center-access-control
description: Orthogonal access control for EdgionCenter — three independent axes (authentication providers, authz mode, storage backend), the authz/auth/local_auth/db_auth/database/audit config, the permission catalog, RBAC enforcement, unified login, admin bootstrap, the audit log, and known limitations.
---

# Standalone access control (three orthogonal axes)

This page applies to `edgion-center-standalone`. Kubernetes mode has a fixed security
composition: OIDC authentication plus Kubernetes SubjectAccessReview authorization, with
no password or database user store.

Center's access control is NOT a single "mode". It is three independent axes that
combine freely:

1. **Authentication** — *who you are*. Any subset of three providers, all active
   simultaneously: OIDC/Okta (`auth:`), a single shared admin (`local_auth:`),
   DB-backed users (`db_auth.enabled`).
2. **Authorization** — *what you may do* (`authz.mode`): `allow_all` (default) or `rbac`.
3. **Storage** — *where state lives* (`database.backend`): `sqlite` (default) or `mysql`.

Authentication and authorization are decoupled: whichever provider issued the token,
RBAC resolves permissions from the database by the authenticated **subject** (OIDC `sub` /
DB username / single-admin name).

Authoritative config example: [`config/edgion-center.yaml`](../../config/edgion-center.yaml).
Code of record: `bins/edgion-center-standalone/src/config/mod.rs` (configuration),
`bins/edgion-center-standalone/src/cli/mod.rs` (composition and bootstrap),
`crates/center-app/src/common/authz/` (catalog and middleware), and
`crates/center-adapter-sql/src/users.rs` (RBAC store).

## The three axes at a glance

| Axis | Key | Values | Default |
|------|-----|--------|---------|
| Authentication: OIDC | `auth.enabled` | on/off (with `discovery`) | off (section absent) |
| Authentication: single admin | `local_auth` (username + password) | on/off | off (section absent) |
| Authentication: DB users | `db_auth.enabled` | on/off | off |
| Authorization | `authz.mode` | `allow_all` \| `rbac` | `allow_all` |
| Storage | `database.backend` | `sqlite` \| `mysql` | `sqlite` |

Authentication is mandatory: with NO provider enabled, business routes fail-close with
**503** (`/health`, `/ready`, `/metrics`, `/api/v1/auth/status` stay reachable). The default
config (allow_all + no db_auth) reproduces the old lightweight "login = admin" behavior.

## Combination matrix (representative)

| Authentication | `authz.mode` | DB required | Behavior |
|----------------|--------------|-------------|----------|
| OIDC only | `allow_all` | Yes | SSO login; every authenticated caller is a full admin. |
| DB users (`db_auth`) | `rbac` | Yes | Username/password login; permissions per user from the `users`/`roles` tables. |
| OIDC | `rbac` | Yes | Okta login, **DB-driven permissions**: each OIDC `sub` must be pre-provisioned as a `users` row, else 403 everywhere. |
| DB users (`db_auth`) | `allow_all` | Yes | Username/password login; every authenticated DB user is a full admin (no per-user keys). |
| single admin + `db_auth` | either | Yes (db_auth) | Unified `/login` authenticates DB users first, then the single shared admin. |

Any combination is valid as long as the startup rules below are satisfied. OIDC + single
admin + db_auth can all be on at once.

## Config keys

### `authz:`
| Key | Default | Meaning |
|-----|---------|---------|
| `mode` | `allow_all` | `allow_all` = login = admin (everyone authenticated gets every key). `rbac` = DB-backed per-subject permissions; REQUIRES a usable database (startup fails without one). |

### `auth:` (OIDC / Okta authentication)
| Key | Default | Meaning |
|-----|---------|---------|
| `enabled` | `true` (when section present) | Enable OIDC bearer-token auth. |
| `discovery` | — | OIDC discovery URL (required when enabled). |
| `audiences` | `[]` | Expected `aud` values; empty = skip audience check. |
| `issuers` | `[]` | Expected `iss` values; empty = validate against the discovery `issuer`. |

(`auth:` also carries algorithm/JWKS/skew/body-limit tuning — see `AdminAuthConfig`.)
OIDC coexists with `rbac` and `db_auth`; there is no force-disable.

### `local_auth:` (single shared admin authentication)
| Key | Default | Meaning |
|-----|---------|---------|
| `username` | `admin` | Single-admin login name. A usable single admin needs a non-empty username AND password. |
| `password` | `""` | Single-admin password (empty = single admin not configured). |
| `jwt_secret` | `""` | Session signing secret; the **preferred** session secret (see precedence below). |
| `jwt_expiry_hours` | `24` | Token lifetime. |
| `cookie_secure` | `true` | Emit the `Secure` cookie attribute. |

### `db_auth:` (DB-backed user authentication)
| Key | Default | Meaning |
|-----|---------|---------|
| `enabled` | `false` | Enable username/bcrypt-password login against the `users` table. REQUIRES a usable database. |
| `jwt_secret` | `null` | Session signing secret; used only when `local_auth.jwt_secret` is unset. |
| `jwt_expiry_hours` | `null` (→24) | Token lifetime on the DB-users-only path (when no `local_auth` admin supplies it). |
| `cookie_secure` | `null` (→true) | `Secure` cookie attribute on the DB-users-only path. |

**Session-secret precedence**: `local_auth.jwt_secret` wins if set, else `db_auth.jwt_secret`.
A session secret is REQUIRED whenever any password login (single admin and/or db_auth) is
enabled — startup fails otherwise. OIDC needs no session secret.

### `database:`
| Key | Default | Meaning |
|-----|---------|---------|
| `enabled` | `true` | Must be true for the standalone binary; database-free operation uses the Kubernetes binary. |
| `backend` | `sqlite` | `sqlite` (embedded) or `mysql` (external). |
| `sqlite_path` | `data/center.db` | SQLite file path (when `backend = sqlite`). |
| `mysql_url` | `null` | `mysql://user:pass@host:3306/db` — REQUIRED when `backend = mysql`; ignored otherwise. A `mysql` backend that fails to connect fails startup (no silent degrade). |

### `audit:`
| Key | Default | Meaning |
|-----|---------|---------|
| `enabled` | `true` | Requires `database.enabled = true`; with no DB, auditing is skipped (WARN at startup). |
| `log_reads` | `false` | Record GET reads too. Mutations are always recorded when enabled. |
| `retention_days` | `0` | Retention window; `0` = keep indefinitely. **Not enforced yet** (see Known limitations). |

The dashboard Audit page reads `GET /api/v1/center/admin/audit-logs` (gated by `audit:read`
under rbac). Attribution works under every combination — the audit log is the compensating
control under `allow_all`, where everyone is an admin.

## Permission catalog

Source of truth: `crates/center-app/src/common/authz/catalog.rs` (`catalog_groups()` / `all_keys()`). **Eleven
keys in seven groups.** Each business API route + dashboard page maps to exactly one key;
roles bundle keys; users get roles.

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

## How RBAC enforcement works (`authz.mode: rbac`)

1. `route_permission(method, path)` in `catalog.rs` maps the concrete request to its required
   key (the single source of truth consulted by the authz middleware).
2. The authz middleware looks up the caller's keys (`DbAuthz`, backed by the `users`/`roles`
   tables, keyed by the authenticated subject) and returns **403** if the required key is
   absent.
3. **Fail-closed for unmapped subjects**: an identity (OIDC `sub` / DB username / admin name)
   with no matching `users` row gets ZERO permissions — 403 everywhere. There is no default
   role and no implicit grant.
4. **Deny-by-default for unmapped routes**: `is_business_path()` marks every `/api/v1/` path
   except `/api/v1/auth/*` as a business path. An unmapped business route denies by default,
   so a newly added route never leaks access before it is added to the catalog.
5. `/auth/me` returns the caller's effective keys; the dashboard gates menus by them.

Under `authz.mode: allow_all` the installed store is `AllowAllAuthz`: every authenticated
caller reports the full key set, so all menus/routes are available (login = admin).

## Unified login order

`POST /api/v1/auth/login` is mounted whenever any password login is active (single admin
and/or db_auth). It authenticates **DB users first, then the single shared admin**. OIDC uses
its own bearer-token flow and does not go through this route.

## Startup validation (fail-close, no silent fallback)

`validate_access` (in `bins/edgion-center-standalone/src/cli/mod.rs`) rejects the boot when:

- `authz.mode = rbac` but there is no usable database.
- `db_auth.enabled = true` but there is no usable database.
- any password login (single admin or db_auth) is on but neither `local_auth.jwt_secret` nor
  `db_auth.jwt_secret` is set.

Having NO authentication provider at all is not a startup error: business routes return 503 at
request time (with a WARN logged at startup).

## First-run admin bootstrap (db_auth)

When `db_auth.enabled` is true AND the `users` table is empty, `bootstrap_admin` reads:

```
EDGION_ADMIN_USERNAME=admin
EDGION_ADMIN_PASSWORD=<strong-password>
```

If both are set, Center creates the admin user (bcrypt). Under `rbac` it also creates a
built-in `admin` role holding every permission key and binds the user to it (one transaction,
all-or-nothing). If either var is unset, a WARN is logged and no DB user can log in until one
is provisioned. Idempotent: a no-op once any user exists.

## Dashboard menu visibility

`GET /api/v1/server-info` returns `authzMode` and `dbAuthEnabled` (it no longer returns
`accessMode`). The dashboard shows:

- the **Users** page when `rbac || db_auth` (managing DB users is meaningful in both),
- the **Roles** page only when `rbac` (roles bundle permission keys).

## Known limitations

- **`retention_days` is not enforced**: audit pruning is not yet scheduled, so records are
  never auto-deleted regardless of the configured value.
- **~30s permission staleness**: `DbAuthz` caches per-subject key sets with a ~30s TTL
  (`CACHE_TTL` in `db_authz.rs`), so role/permission changes take up to ~30s to take effect.
- **Single-Center only**: DB-backed RBAC assumes one Center (no cross-Center sync of
  users/roles/permissions).

## Deeper detail

- Orthogonal design: [`docs/history/superpowers/specs/2026-06-14-center-orthogonal-access-control-design.md`](../../docs/history/superpowers/specs/2026-06-14-center-orthogonal-access-control-design.md)
- Prior (superseded) dual-tier design: [`docs/history/superpowers/specs/2026-06-13-center-dual-access-control-design.md`](../../docs/history/superpowers/specs/2026-06-13-center-dual-access-control-design.md)
