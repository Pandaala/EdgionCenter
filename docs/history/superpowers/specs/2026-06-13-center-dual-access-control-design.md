# Design: Center dual-tier access control (lite / full, config-selected)

> **SUPERSEDED (2026-06-14):** The bundled `access.mode: lite | full` switch introduced here
> has been replaced by the **orthogonal** model in
> [`2026-06-14-center-orthogonal-access-control-design.md`](2026-06-14-center-orthogonal-access-control-design.md):
> three independent axes — authentication (OIDC / single-admin / DB users, any subset),
> authorization (`authz.mode: allow_all | rbac`), and storage (`database.backend`). Only the
> `access.mode` tier selector is retired; the rest of this design (sqlx Store, audit log, RBAC
> permission-key engine, dashboard Users/Roles) still stands and was carried forward.

**Profile:** design / feature (multi-task)
**Status:** approved — ready for implementation plan (access.mode tier selector superseded; see note above)
**Date:** 2026-06-13
**Supersedes / consolidates:** `docs/history/tasks/center-auth-rbac-design.md`,
`docs/history/tasks/center-audit-log.md` (those drafts pushed RBAC into a separate
"Management Plane"; this design keeps **both tiers in the single Center codebase**, selected
by config, per the product decision).

## Goal

One `edgion-center` codebase that supports two mutually-exclusive access-control tiers,
chosen at startup by a config switch:

- **lite** — lightweight. Authentication via Okta/OIDC (plus the existing single-admin
  `local_auth`). Authorization is `login = admin` (no per-user RBAC). User-attributed **audit
  log**. Default storage SQLite.
- **full** — feature-rich. DB-backed local users (username + password), **page- and
  API-endpoint-level RBAC** fully manageable from the UI, audit log. Default storage MySQL
  (SQLite also acceptable for a single node).

Nothing in this design touches the Controller side; Center is assumed to have all required
permissions on downstream controllers.

## Decisions (locked)

- Permission granularity (full): **page + API endpoint level** via a code-defined
  permission-key catalog. NOT per-controller scope, NOT full verb×kind×scope.
- **Single Center.** No multi-Center user/permission sync (dropped from the old drafts).
- lite tier authz: **pure `login = admin` + audit**. No Okta-group→role mapping.
- Storage: **unified on `sqlx`**, one `Store` trait with SQLite and MySQL backends. The
  existing `rusqlite` direct-access code is **deleted** and the `controllers` table migrated.
- full tier authn: **pure DB local users** (no Okta in full mode).
- Password hashing: **bcrypt** (consistent with existing `local_auth`).
- Task files: one per todo under `docs/history/tasks/` with a `Status` frontmatter line.

## Current state (baseline, verified)

- **Authn** exists: `src/common/unified_auth/` orchestrates OIDC (`src/common/auth/`) and
  single-admin `local_auth` (`src/common/local_auth/`); fail-close 503 when no provider ready,
  401 on bad token. Skip-paths allowlist for `/health`, `/ready`, `/metrics`, `/api/v1/auth/*`.
- **No authz**: every authenticated request is full-admin. No users/roles/permissions.
- **No audit log**: only `tracing` events.
- **Storage**: `src/db/mod.rs` uses `rusqlite` directly, single `controllers` table, embedded
  `SCHEMA_SQL`, no trait abstraction, SQLite-only (`ON CONFLICT` dialect). MySQL-hostile.
- **HTTP**: Axum 0.8; business routes in `src/api/mod.rs`; auth composition in
  `src/common/api/compose.rs` (`compose_admin_routes` wraps business with `unified_auth`).
- **Frontend** (`web/`): login page + `RequireAuth` route guard + 401→/login interceptor;
  no user/role/audit pages. Login state flag in `sessionStorage`.

## Architecture

### Config switch

New top-level block selecting the tier:

```yaml
access:
  mode: lite        # "lite" | "full"
```

- `lite`: requires `[auth]` (OIDC) and/or `[local_auth]` (existing). authz = allow-all.
- `full`: requires `[database]` with users provisioned; DB-user authn; DB RBAC. `[auth]`/
  `[local_auth]` ignored for login (full is DB-users only).

Validation: `full` mode with no usable `[database]` fails startup with a clear error. `lite`
with neither `[auth]` nor `[local_auth]` keeps the existing fail-close 503 on business routes.

### Storage layer (sqlx)

```
[database]
backend     = "sqlite"   # "sqlite" | "mysql"
sqlite_path = "data/center.db"
mysql_url   = "mysql://user:pass@host:3306/edgion_center"   # used when backend = mysql
```

- A `Store` trait exposes all persistence: `controllers` CRUD (existing behavior preserved),
  `audit_log` insert/list/prune, and (full only) `users`/`roles`/`bindings` CRUD + lookups.
- Two implementations behind one async `sqlx` API. Dialect differences (upsert) handled per
  backend. Migrations via `sqlx::migrate!` with backend-appropriate SQL.
- `rusqlite` dependency and `src/db/mod.rs` direct code are removed; callers
  (`src/api/mod.rs`, `src/fed_sync/server/mod.rs`, `src/cli/mod.rs`) switch to the trait.

### Authz abstraction

```
trait AuthzStore { fn permissions_for(&self, principal: &Principal) -> PermissionSet; }
trait AuthzAdmin { /* create/update/delete users, roles, bindings — full only */ }
```

- `AllowAllAuthz` (lite): `permissions_for` returns the full permission-key set; the authz
  middleware becomes a pass-through.
- `DbAuthz` (full): resolves principal → roles → permission-key set from the DB (cached with
  short TTL / invalidation on mutation).

A new **authz middleware** sits *inside* `unified_auth` (claims already injected). It maps the
incoming `(method, path)` to the route's declared permission key and rejects with 403 if the
principal's set lacks it. lite mode short-circuits (allow-all).

### Permission model (page + API, capability keys)

- A **code-defined catalog** of permission keys, e.g. `controllers:read`, `controllers:write`,
  `region-routes:read`, `region-routes:write`, `ip-restrictions:write`, `audit:read`,
  `users:manage`, `roles:manage`. Each is grouped under a page/feature for UI display.
- Each API route is annotated with its required permission key. Each frontend page/menu maps
  to one or more keys.
- DB stores **bindings only**: `role_permissions` (role → keys), `user_roles` (user → roles).
  The catalog itself is code, so adding a route never needs a data migration.
- `GET /api/v1/auth/me` returns the caller's effective permission-key set (full set in lite)
  so the UI can gate menus/buttons. Backend middleware is the real enforcement; UI gating is
  UX only.

### Database schema

`controllers` (migrated, unchanged columns). `audit_log` (both tiers). full-only:
`users`, `roles`, `user_roles`, `role_permissions`, optional `api_tokens`.

```sql
-- audit_log (both tiers)
id, ts, actor, provider, method, path, target_controller,
status, source_ip, request_id, detail

-- full tier
users            (id, username UNIQUE, password_hash, display_name, status, created_at, updated_at)
roles            (id, name UNIQUE, description, created_at, updated_at)
user_roles       (user_id, role_id)                  -- PK(user_id, role_id)
role_permissions (role_id, permission_key)           -- PK(role_id, permission_key)
api_tokens       (id, user_id, token_hash, name, created_at, last_used_at)   -- optional
```

### Audit middleware

- Placed inside `unified_auth`, outside business routes, so `UnifiedAuthClaims` is present.
- Reads claims (`sub`/username, `provider`), runs the handler, captures status; `source_ip`
  from `ConnectInfo<SocketAddr>` (**never** X-Forwarded-For).
- Writes via a bounded `mpsc` + background writer task; the request never blocks on the DB.
  Full channel → drop + increment a dropped-record metric (**fail-open**: audit is a
  compensating control, not the primary gate). Login success / logout emitted from the auth
  handlers (failed logins are a noted follow-up — they are rejected outside the inner layer).
- Audits mutating ops (`POST/PUT/DELETE/PATCH`) and proxied controller ops (decode the target
  `controller_id`). `GET` reads off by default behind `audit.log_reads`.
- Read API: `GET /api/v1/center/admin/audit-logs` (paginated; filters: actor, controller,
  since/until). Excluded from `log_reads` to avoid self-logging loops.

```yaml
audit:
  enabled: true        # default true when database enabled
  log_reads: false
  retention_days: 0    # 0 = unbounded (+ WARN); optional pruning
```

### Frontend

- New pages: **Audit Log** (both tiers), **User Management** + **Role/Permission matrix**
  (full only). Permission matrix = checkbox grid of catalog keys grouped by page.
- Menu/button gating from `/auth/me` permission keys (lite = full set → everything visible).
- Login: full mode uses the existing username/password login UI against DB users; lite uses
  Okta/local as today.

## Error handling

- Unknown/invalid `access.mode` → startup error.
- `full` with no usable database → startup error (do not silently fall back to lite).
- Authz middleware: missing permission → 403 (distinct from 401 missing/invalid auth, 503 not
  ready). Permission catalog must cover every business route; a route with no declared key is
  treated as deny in full mode (caught by a startup completeness check / test).
- Audit write failure → fail-open + metric + WARN.
- DB user with `status != active` → login rejected.

## Testing

- Storage: controllers CRUD round-trip on both SQLite and MySQL backends; migration idempotency.
- Authz: middleware allows/denies per permission key; lite allow-all pass-through; startup
  completeness check that every business route has a declared permission key.
- Audit: middleware records correct actor/status/source_ip; fail-open on full channel; reads
  excluded by default.
- Full authn: bcrypt verify; inactive user rejected; `/auth/me` returns correct key set.
- Frontend: route guard, menu gating by keys, role-matrix CRUD against a mock API.

## Implementation order (each task independently testable)

1. **Storage foundation** — add `sqlx` + `Store` trait + `[database] backend` selection,
   migrate `controllers`, delete `rusqlite`, regression-verify controllers API.
2. **Audit backend** — `audit_log` schema + `Store` methods + audit middleware + background
   writer + `[audit]` config.
3. **Audit read API + frontend audit page**.
4. **Authz abstraction + `[access] mode`** — `AuthzStore` trait, `AllowAllAuthz`, authz
   middleware wiring, `/auth/me` returns permission keys. (lite tier fully deliverable here.)
5. **full schema + permission-key catalog** — users/roles/bindings tables + per-route key
   annotations + startup completeness check.
6. **DbAuthz + DB-user authn provider** — login, bcrypt, permission resolution + enforcement.
7. **User/role admin CRUD API** — `/api/v1/center/admin/users`, `/roles`, bindings.
8. **Frontend user-management + role/permission-matrix pages + menu gating**.
9. **Wrap-up** — example configs for both modes, skills docs, migration notes.

## Out of scope

- Multi-Center sync (dropped).
- Per-controller / verb×kind×scope authorization.
- Okta-group→role mapping; Okta login in full mode.
- Server-side OIDC token revocation; SSO session management beyond current JWT/cookie.
- Any Controller-side change.

## References

- Authn: `src/common/unified_auth/mod.rs`, `src/common/auth/oidc.rs`,
  `src/common/local_auth/handlers.rs`
- Router composition: `src/common/api/compose.rs`, `src/api/mod.rs`, `src/cli/mod.rs`
- Existing DB (to be replaced): `src/db/mod.rs`, `src/config/mod.rs` (`DatabaseConfig`)
- IP/peer handling (source_ip, no XFF): `src/common/api/ip_allowlist.rs`
- Prior drafts: `docs/history/tasks/center-auth-rbac-design.md`,
  `docs/history/tasks/center-audit-log.md`
