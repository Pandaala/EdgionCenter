# Design: Orthogonal access control (drop `access.mode`, free-combine authn/authz/storage)

**Profile:** design / refactor
**Status:** approved — ready for implementation plan
**Date:** 2026-06-14
**Refactors:** the `access.mode: lite | full` bundled switch introduced by
`docs/history/superpowers/specs/2026-06-13-center-dual-access-control-design.md` (and the
DAC-01..09 implementation). All other pieces (sqlx Store, audit, RBAC engine, dashboard pages)
are kept; only the *configuration model and provider assembly* change.

## Goal

Replace the single bundled `access.mode` selector with three **independent, freely-combinable
axes**, so operators can mix authentication, authorization, and storage as they like:

- **Authentication (authn)** — any subset of three providers enabled simultaneously:
  OIDC/Okta (`auth`), single shared admin (`local_auth`), DB users (`db_auth`, new).
- **Authorization (authz)** — `authz.mode: allow_all | rbac` (explicit).
- **Storage** — `database.backend: sqlite | mysql` (already independent; unchanged).

Any combination is valid, e.g. `OIDC + rbac`, `DB-users + allow_all`, `OIDC + local-admin + rbac`.

## Locked decisions

- Unmapped principal under `rbac` (an authenticated identity with no matching `users` row /
  role bindings — e.g. an un-provisioned OIDC user, or the single admin) → **empty permission
  set → 403 on every business route (fail-closed)**. No default role, no admin exception.
- The authz axis is an **explicit** `authz.mode: allow_all | rbac` (default `allow_all`).
- HS256 session secret precedence: `local_auth.jwt_secret` first, else `db_auth.jwt_secret`.
- Unified password login order: **DB users first, then the local single-admin** (break-glass).
- OIDC users under `rbac` must be pre-provisioned as `users` rows (username = OIDC `sub`); the
  `password_hash` on such mapping-only rows is unused (placeholder is fine).

## Current state being refactored

- `src/config/mod.rs`: `AccessConfig { mode: AccessMode (Lite|Full) }` on `CenterConfig`.
- `src/cli/mod.rs`: a `match config.access.mode` arm picks `AllowAllAuthz` (lite, with
  OIDC+single-admin login) vs `DbAuthz` (full, with DB login, OIDC force-disabled, random
  placeholder admin password, fail-startup without DB+jwt_secret).
- `src/common/db_auth/` mounts a DB-only login in place of `local_auth`'s single-admin login.
- `src/api/mod.rs` server-info returns `accessMode: "lite"|"full"`; threaded via `ApiState`.
- Frontend `menuConfig.tsx` gates Users/Roles on `requiredMode: 'full'` + `accessMode` from
  server-info.
- Authn (`unified_auth`), authz engine (`AuthzStore`/`DbAuthz`/`AllowAllAuthz`/catalog/
  middleware), Store (users/roles), CRUD API, and the dashboard pages are all reused unchanged.

## Architecture

### Config surface

Remove `AccessConfig`/`AccessMode` and the `access:` block. Add:

```rust
// authz axis
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all="snake_case")]
pub enum AuthzMode { AllowAll, Rbac }                 // default AllowAll
#[derive(Serialize, Deserialize)] #[serde(default)]
pub struct AuthzConfig { pub mode: AuthzMode }
// add: pub authz: AuthzConfig on CenterConfig

// DB-user authn provider
#[derive(Serialize, Deserialize)] #[serde(default)]
pub struct DbAuthConfig {
    pub enabled: bool,                  // default false
    pub jwt_secret: Option<String>,     // used only when local_auth absent
    pub jwt_expiry_hours: Option<u64>,  // falls back to local_auth's / default 24
    pub cookie_secure: Option<bool>,    // falls back to local_auth's / default true
}
// add: pub db_auth: DbAuthConfig on CenterConfig
```

`auth` (OIDC) and `local_auth` (single-admin + session settings) keep their current structs.

```yaml
authz:
  mode: allow_all          # allow_all (default) | rbac
auth:                      # optional OIDC
  discovery: "..."
local_auth:                # optional single-admin + HS256 session settings
  username: admin
  password: ...
  jwt_secret: ...
db_auth:                   # optional DB-user login
  enabled: true
database:
  backend: sqlite          # sqlite | mysql
```

### Authentication assembly (`unified_auth`, cli wiring)

`unified_auth` accepts tokens from every enabled provider:
- **OIDC** (when `auth` present & enabled): RS256 validation (existing OIDC provider).
- **HS256 local validator** (when `local_auth` and/or `db_auth` enable a password login): one
  validator keyed by the resolved session secret. Both single-admin and DB-user logins issue
  HS256 tokens signed with that one secret; `sub` = username.

**Session secret resolution** (one secret, shared): `local_auth.jwt_secret` if `local_auth`
configured, else `db_auth.jwt_secret`. Required whenever any password login is enabled; absent →
startup error. OIDC needs no secret.

**Unified password login** at `POST /api/v1/auth/login`: a single handler that, in order,
(1) if `db_auth.enabled`, looks up the DB user (bcrypt, reject inactive); (2) else/also if
`local_auth` has credentials, checks the single admin. First match issues the token via the
shared `issue_login_response` helper. Uniform 401 (timing-safe dummy bcrypt when no source
matches). `logout`/`me` reused unchanged. This replaces the current "mount db-login XOR
single-admin-login by mode" split.

### Authorization assembly

- `authz.mode = allow_all` → `AllowAllAuthz` (every authenticated caller = all keys).
- `authz.mode = rbac` → `DbAuthz` over the Store, resolving `principal.subject` →
  `permission_keys_for_user` (already filters `status='active'`). Unmapped/empty → deny.
  Requires `database.enabled` + a reachable Store (startup error otherwise).

The authz middleware, catalog, deny-by-default for unmapped business routes, and `/auth/me`
permission injection are all unchanged — they already key purely off the injected claims +
`AuthzStore`, independent of which provider authenticated the request.

### Removed coupling

Delete: the `access.mode` match, the "full disables OIDC + WARN" branch, and the random
placeholder-admin-password trick. OIDC now coexists with `rbac`/`db_auth` freely. Replace the
mode match with provider-driven assembly: build the provider set from `auth`/`local_auth`/
`db_auth`, the `AuthzStore` from `authz.mode`, and validate the combination at startup.

### Bootstrap (first-run admin)

Runs when `db_auth.enabled` AND `users` table empty AND `EDGION_ADMIN_USERNAME`/`PASSWORD`
set: create the admin user (atomic `Store::bootstrap_admin`). When `authz.mode = rbac`, the
same atomic call also creates the `admin` role with `all_keys()` and binds it. If `db_auth`
enabled but env unset → WARN (no login possible). (If `rbac` is on but `db_auth` off, identities
are provisioned out-of-band/by OIDC mapping; bootstrap only WARNs.)

### Frontend / server-info

`GET /api/v1/server-info` replaces `accessMode` with `authzMode: "allow_all"|"rbac"` and
`dbAuthEnabled: bool` (threaded via `ApiState`). Menu gating predicate (`menuConfig.tsx`):
- **Users page** visible iff `dbAuthEnabled || authzMode === 'rbac'` (the `users` table is in
  use), AND the existing `users:manage` permission key.
- **Roles / permission-matrix page** visible iff `authzMode === 'rbac'`, AND `roles:manage`.
- Audit page: unchanged (`audit:read`).
Replace the `requiredMode: 'full'` field with a small `requiredAccess?: { authzMode?: 'rbac';
dbAuth?: true }`-style gate (or two booleans the predicate checks). Login page needs no change
(the username/password endpoint serves both local-admin and DB users; OIDC is bearer-token).

## Error handling / validation (startup)

- `authz.mode = rbac` with no usable database → startup error.
- Any password login enabled (`local_auth` creds or `db_auth.enabled`) with no resolvable
  session secret → startup error.
- No authn provider enabled at all → business routes keep returning 503 (existing fail-close);
  `/health`,`/ready`,`/metrics`,`/api/v1/auth/*` stay reachable.
- `db_auth.enabled` with no usable database → startup error (DB users need the `users` table).

## Testing

- Config parse: each axis independently; defaults (authz allow_all, db_auth disabled).
- Assembly: matrix of representative combos builds a working app — `OIDC+allow_all`,
  `db_auth+rbac`, `OIDC+rbac` (unmapped OIDC sub → 403; provisioned → 200), `db_auth+allow_all`
  (DB login but all-keys), `local_auth+db_auth` (unified login tries DB then admin).
- Startup validation: rbac without DB → error; password login without secret → error.
- Unified login: DB user wins; falls back to single-admin; uniform 401; timing-safe.
- server-info reports authzMode + dbAuthEnabled; menu predicate hides Users/Roles appropriately
  (allow_all + no db_auth → both hidden; rbac → both shown; db_auth + allow_all → Users shown,
  Roles hidden).
- Existing RBAC enforcement / audit / Store tests stay green.

## Implementation order (each independently testable)

1. **Config refactor** — remove `AccessConfig/AccessMode`; add `AuthzConfig/AuthzMode` +
   `DbAuthConfig`; parse tests. (Compiles with cli temporarily adapted.)
2. **Provider-driven cli assembly** — build authn provider set + `AuthzStore` from the new
   config; remove the `access.mode` match, OIDC-disable, placeholder-password; startup
   validation; bootstrap retie. Assembly/validation tests.
3. **Unified password login** — one `/auth/login` handler over db_auth + local_auth (DB-first,
   admin fallback); reuse logout/me. Login tests (both sources, fallback, uniform 401).
4. **server-info fields** — `authzMode` + `dbAuthEnabled` replace `accessMode`; handler test.
5. **Frontend gating** — new predicate + `useServerInfo` fields; Users/Roles visibility per the
   matrix; update menu tests.
6. **Docs** — rewrite `config/edgion-center.yaml` access-control section + `skills/02-features/
   access-control.md` for the orthogonal model; mark the 2026-06-13 spec's `access.mode` as
   superseded by this design.

## Out of scope

- A configurable default role for unmapped principals (explicitly rejected — fail-closed).
- OIDC interactive login flow in the dashboard (unchanged; bearer-token model).
- Multi-Center sync, per-controller scope, verb×kind authz (still out per the prior spec).
- Any Controller-side change.

## References

- Prior (bundled) design: `docs/history/superpowers/specs/2026-06-13-center-dual-access-control-design.md`
- Config: `src/config/mod.rs`; assembly: `src/cli/mod.rs`; login: `src/common/db_auth/`,
  `src/common/local_auth/handlers.rs`; authz: `src/common/authz/`; server-info: `src/api/mod.rs`;
  frontend gating: `web/src/components/shell/menuConfig.tsx`, `web/src/hooks/useServerInfo.ts`.
