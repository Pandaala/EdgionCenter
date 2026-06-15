# Orthogonal Access Control — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the bundled `access.mode: lite|full` switch with three independent,
freely-combinable axes — authn providers (OIDC / local-admin / DB-users), `authz.mode:
allow_all|rbac`, and `database.backend` — so any combination is configurable.

**Architecture:** Delete `AccessConfig/AccessMode`; add `AuthzConfig{mode}` + `DbAuthConfig`.
Replace the `match access.mode` cli arm with provider-driven assembly: build the authn provider
set from `auth`/`local_auth`/`db_auth`, pick the `AuthzStore` from `authz.mode`, and serve a
single unified `/auth/login` that authenticates DB users then the single admin. server-info and
the dashboard menu gate on `authzMode` + `dbAuthEnabled` instead of `accessMode`.

**Tech Stack:** Rust, Axum 0.8, sqlx, bcrypt, React + TypeScript + Vite.

**Spec:** `docs/history/superpowers/specs/2026-06-14-center-orthogonal-access-control-design.md`

**Conventions (unchanged from prior work):** binary crate → `cargo test --bin edgion-center
<filter>` (no `--lib`); frontend → `cd web && npm run test` / `npm run build`; commit trailer
`Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`; runtime sqlx only.

---

## File Structure

- `src/config/mod.rs` — **modify:** remove `AccessConfig`/`AccessMode`; add `AuthzMode`,
  `AuthzConfig`, `DbAuthConfig`; wire `authz` + `db_auth` onto `CenterConfig`.
- `src/common/local_auth/handlers.rs` — **modify:** expose a timing-safe single-admin verify so
  the unified login can reuse it (factor from the existing `login_handler`).
- `src/common/db_auth/mod.rs`, `src/common/db_auth/handlers.rs` — **modify:** replace the
  DB-only login with a **unified** login handler (DB users → single-admin fallback) + a route
  mounter that takes the provider set.
- `src/cli/mod.rs` — **modify:** replace the `match access_mode` block with provider-driven
  assembly + startup validation + bootstrap retie.
- `src/api/mod.rs` — **modify:** `ApiState` carries `authz_mode` + `db_auth_enabled`;
  server-info returns `authzMode` + `dbAuthEnabled` (drop `accessMode`).
- `web/src/hooks/useServerInfo.ts`, `web/src/api/client.ts` — **modify:** new server-info fields.
- `web/src/components/shell/menuConfig.tsx`, `Sidebar.tsx` — **modify:** replace `requiredMode`
  gate with `authzMode`/`dbAuth` gate.
- `config/edgion-center.yaml`, `skills/02-features/access-control.md` — **modify:** rewrite for
  the orthogonal model.

---

## Task 1 — Config refactor

**Files:** Modify `src/config/mod.rs`.

**Interfaces:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthzMode { #[default] AllowAll, Rbac }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthzConfig { pub mode: AuthzMode }          // Default { mode: AllowAll }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DbAuthConfig {
    pub enabled: bool,                  // default false
    pub jwt_secret: Option<String>,     // used only when local_auth is absent
    pub jwt_expiry_hours: Option<u64>,
    pub cookie_secure: Option<bool>,
}
// CenterConfig: remove `access: AccessConfig`; add `pub authz: AuthzConfig` and
// `pub db_auth: DbAuthConfig` (both #[serde(default)]); update Default + the struct's tests.
```

**Steps:**
- [ ] **1.1** Delete `AccessMode`/`AccessConfig` and the `access` field from `CenterConfig`
  (+ its `Default`). Add `AuthzMode`/`AuthzConfig`/`DbAuthConfig` and the `authz`/`db_auth`
  fields. The crate won't compile yet (cli references `access`) — that's fixed in Task 3.
- [ ] **1.2** Add parse tests: `authz_mode_defaults_allow_all` (empty yaml → AllowAll),
  `authz_mode_rbac_parses` (`authz:\n  mode: rbac`), `db_auth_defaults_disabled`,
  `db_auth_enabled_parses` (`db_auth:\n  enabled: true\n  jwt_secret: "x"`). Use the existing
  `singleton_map_recursive` production deserializer pattern already in this file's tests.
- [ ] **1.3** Run `cargo test --bin edgion-center config 2>&1 | tail` — it won't fully build
  until Task 3; instead verify just this module compiles via `cargo check` is also blocked.
  **Sequencing note:** Tasks 1→3 must land together to compile. Implement 1, then 2, then 3,
  and run the build/tests at the end of Task 3. Commit Task 1 only if the tree compiles;
  otherwise fold the Task 1 commit into Task 3 (note this in the report).

**Acceptance:** new config types parse; `access` gone. (Build green only after Task 3.)

---

## Task 2 — Unified password login handler

**Files:** Modify `src/common/local_auth/handlers.rs`, `src/common/db_auth/{mod,handlers}.rs`.

**Goal:** One `POST /api/v1/auth/login` that authenticates against DB users first, then the
local single-admin, issuing the same HS256 token. Reuse `logout`/`me`.

**Interfaces:**
```rust
// src/common/local_auth/handlers.rs — factor a reusable, timing-safe single-admin check
// from the existing login_handler. Returns true iff (username,password) match the configured
// single admin; ALWAYS runs a bcrypt verify (real-or-dummy) to equalize timing.
impl LocalAuthState { pub(crate) fn verify_single_admin(&self, username: &str, password: &str) -> bool; }
// keep issue_login_response(local_state, username) as the shared token/cookie issuer.

// src/common/db_auth/mod.rs
pub struct UnifiedLoginState {
    pub store: Option<std::sync::Arc<crate::store::Store>>, // Some when db_auth enabled
    pub local: std::sync::Arc<crate::common::local_auth::LocalAuthState>, // HS256 issuer/validator
    pub single_admin_enabled: bool,                         // local_auth has real creds
}
// handlers.rs: unified_login_handler — order:
//   1. if let Some(store) = state.store: get_user_by_username; if found && status=="active"
//      && bcrypt verify(password, hash) → issue_login_response. (Always bcrypt-verify a
//      dummy hash when the user is missing — timing.)
//   2. else/also if state.single_admin_enabled && state.local.verify_single_admin(u,p) → issue.
//   3. otherwise uniform 401 (run a dummy bcrypt so the no-source path is constant-time too).
// add_unified_auth_routes(business, login_state, local_state) mounts POST /auth/login
//   (unified) + reuses the existing local logout/me handlers. (Replaces add_db_auth_routes.)
```

**Steps:**
- [ ] **2.1** Factor `verify_single_admin` out of `local_auth::login_handler` (keep the existing
  endpoint working — have it call the new helper). Unit test `verify_single_admin_*` (right
  creds true; wrong false; missing/dummy timing path doesn't panic).
- [ ] **2.2** Failing tests for `unified_login_handler`:
  - `unified_login_db_user_wins` (store with active user `alice` → login alice ok, cookie set).
  - `unified_login_falls_back_to_admin` (store present but no such user; single admin matches → ok).
  - `unified_login_db_inactive_rejected` (disabled user → 401 even though creds right).
  - `unified_login_uniform_401` (neither source matches → 401).
  - `unified_login_no_store_admin_only` (store None, single admin only → admin login ok).
- [ ] **2.3** Implement `UnifiedLoginState` + `unified_login_handler` + `add_unified_auth_routes`.
  Run the tests → PASS.
- [ ] **2.4** Commit (or fold into Task 3 if the tree isn't yet compiling):
  `feat: unified password login (DB users then single-admin)`.

**Acceptance:** one login endpoint authenticates DB users and the single admin; uniform 401;
timing-safe; inactive DB users rejected.

---

## Task 3 — Provider-driven cli assembly + validation + bootstrap

**Files:** Modify `src/cli/mod.rs`.

**Goal:** Replace the `match config.access.mode { Lite => .., Full => .. }` block
(`src/cli/mod.rs` ~lines 345-433) with assembly from the new axes.

**Logic:**
```text
let oidc_on        = auth_config (present & enabled);
let single_admin_on= local_auth_config has non-empty username & password;
let db_auth_on     = config.db_auth.enabled;
let rbac           = config.authz.mode == Rbac;

// session secret: local_auth.jwt_secret if set, else db_auth.jwt_secret. Required if any
// password login (single_admin_on || db_auth_on). Absent → startup error.
// db_auth_on requires a usable Store → else startup error.
// rbac requires a usable Store → else startup error.

// UnifiedAuthState: from_configs(oidc?, local_validator?, require_auth, "center")
//   - OIDC passed through ONLY when oidc_on (no more force-disable).
//   - local HS256 validator installed when any password login on (carries the resolved secret,
//     expiry, cookie_secure). When single_admin_on, the validator also carries the real
//     username/password (so verify_single_admin works); when only db_auth_on, the validator
//     carries a random placeholder password (defense-in-depth) — single_admin_enabled=false.

// AuthzStore: rbac → DbAuthz::new(store); else AllowAllAuthz.
// Bootstrap: if db_auth_on && users table empty && EDGION_ADMIN_* set → bootstrap_admin
//   (creates admin user; when rbac, also admin role with all_keys() + binding). Reuse the
//   existing atomic Store::bootstrap_admin. If db_auth_on but env unset → WARN.
// Routes: add_unified_auth_routes(base_router, login_state, local_state) when any password
//   login on; compose with local_auth_intent = (any password login on). When only OIDC (no
//   password login), mount no login route and compose with local_auth_intent=false.
```

**Steps:**
- [ ] **3.1** Remove the `match access_mode` block, the OIDC-disable branch, and the
  random-placeholder-only path; implement the assembly above. Keep the audit-layer wiring and
  the `compose_admin_routes(business, auth_state, local_auth_intent, authz)` call.
- [ ] **3.2** Startup-validation tests (construct configs and assert the cli build result):
  `rbac_without_db_errors`, `db_auth_without_db_errors`, `password_login_without_secret_errors`,
  `oidc_only_allow_all_builds` (no password login, no DB → builds, business routes 401 w/o token).
  If the cli build path is hard to unit-test directly, add the validation as a small pure
  function `fn validate_access(config, store_present) -> anyhow::Result<()>` and test THAT, then
  call it from the assembly. (Preferred — keeps it testable.)
- [ ] **3.3** Update/retire the prior `access.mode`-based cli tests (e.g.
  `center_no_auth_config_admin_routes_return_503` must still pass; any test referencing
  `AccessMode` is updated to the new config).
- [ ] **3.4** `cargo build` + `cargo test --bin edgion-center` → green (Tasks 1+2+3 now compile
  together).
- [ ] **3.5** End-to-end assembly tests (compose the app like the existing
  `db_authz_enforces_per_route_permissions` test): 
  - `oidc_rbac_unmapped_sub_403` — rbac + a (faked) OIDC-style claims sub not in `users` → 403
    on a business route; after seeding a `users` row with that name + a role → 200.
  - `db_auth_allow_all_grants_everything` — db_auth login + allow_all → a DB user reaches a
    write route (200).
  - `db_auth_rbac_enforces` — keep the existing per-route enforcement assertion under the new
    assembly.
- [ ] **3.6** Commit: `feat: provider-driven access-control assembly (orthogonal axes)`.

**Acceptance:** any axis combination assembles; startup validation fails closed; bootstrap works
when db_auth on; OIDC coexists with rbac; lite-equivalent (OIDC/admin + allow_all) unchanged.

---

## Task 4 — server-info fields

**Files:** Modify `src/api/mod.rs`, `src/cli/mod.rs` (ApiState construction).

**Steps:**
- [ ] **4.1** Replace `access_mode: AccessMode` on `ApiState` with `authz_mode: AuthzMode` and
  `db_auth_enabled: bool`. Update every `ApiState` construction (production in `cli` + the
  `state_with_db` test helpers in `api/mod.rs`, `api/audit.rs`, `api/users.rs`, `api/roles.rs`).
- [ ] **4.2** server-info response: drop `accessMode`; add `authzMode: "allow_all"|"rbac"` and
  `dbAuthEnabled: bool` (serde camelCase). Update the handler.
- [ ] **4.3** Update the backend test `server_info_reports_access_mode` →
  `server_info_reports_authz_and_db_auth` asserting both new fields for an rbac+db_auth state and
  an allow_all+no-db_auth state. Run → PASS.
- [ ] **4.4** Commit: `feat: server-info exposes authzMode + dbAuthEnabled`.

**Acceptance:** server-info returns the two new fields; `accessMode` gone.

---

## Task 5 — Frontend gating

**Files:** Modify `web/src/api/client.ts`, `web/src/hooks/useServerInfo.ts`,
`web/src/components/shell/menuConfig.tsx`, `web/src/components/shell/Sidebar.tsx`,
`web/src/components/shell/menuConfig.test.tsx`.

**Steps:**
- [ ] **5.1** `client.ts`: change the `serverInfo` return type — replace `accessMode?` with
  `authzMode?: 'allow_all' | 'rbac'` and `dbAuthEnabled?: boolean`.
- [ ] **5.2** `menuConfig.tsx`: replace the per-item `requiredMode?: 'full'` with
  `requiredAuthz?: 'rbac'` and `requiredDbAuth?: boolean`. Update the predicate:
  ```ts
  // visible iff all gates pass:
  if (item.requiredAuthz && ctx.authzMode !== item.requiredAuthz) return false
  if (item.requiredDbAuth && !ctx.dbAuthEnabled) return false
  if (item.requiredPermission && !ctx.permissions.includes(item.requiredPermission)) return false
  return true
  ```
  Gate Users with `{ requiredPermission: 'users:manage' }` PLUS a combined "users table in use"
  check — since the predicate ANDs gates, model "Users visible when rbac OR db_auth" by giving
  the Users item NO single hard gate but a custom check. Simplest: extend the ctx with a derived
  `userMgmtAvailable = authzMode === 'rbac' || dbAuthEnabled` and gate Users on
  `requiredUserMgmt: true` (checks `ctx.userMgmtAvailable`); gate Roles on `requiredAuthz: 'rbac'`.
  Both keep their `requiredPermission`. (Pick whichever of these two shapes is cleaner; the
  REQUIREMENT is: Users shows when `rbac || dbAuth`, Roles shows only when `rbac`.)
- [ ] **5.3** `Sidebar.tsx`/`useServerInfo.ts`: source `authzMode` + `dbAuthEnabled` from
  server-info and pass them into the filter ctx (replacing `accessMode`).
- [ ] **5.4** Update `menuConfig.test.tsx`: assert Users hidden when `allow_all` + no db_auth;
  Users shown when `db_auth` (even allow_all); Users + Roles shown when `rbac`; Roles hidden when
  `db_auth` + allow_all. Run → PASS.
- [ ] **5.5** `cd web && npm run test && npm run build` → green. Commit:
  `feat: dashboard menu gating on authzMode + dbAuthEnabled`.

**Acceptance:** Users page shows when rbac OR db_auth; Roles page only when rbac; both still
honor their permission keys; build/tests green.

---

## Task 6 — Docs

**Files:** Modify `config/edgion-center.yaml`, `skills/02-features/access-control.md`,
`docs/history/superpowers/specs/2026-06-13-center-dual-access-control-design.md` (mark the
`access.mode` portion superseded).

**Steps:**
- [ ] **6.1** Rewrite the access-control section of `config/edgion-center.yaml`: remove the
  `access:` block; document `authz.mode`, the three authn providers (`auth`/`local_auth`/
  `db_auth`), the session-secret precedence, the unified login, rbac fail-closed for unmapped
  identities, and the bootstrap env vars. Keep YAML valid; default config stays allow_all + no
  db_auth (lite-equivalent).
- [ ] **6.2** Rewrite `skills/02-features/access-control.md` around the orthogonal model: the
  three axes, a combination matrix (OIDC+allow_all, DB+rbac, OIDC+rbac, DB+allow_all,
  local+db_auth), config keys, startup validation, bootstrap, menu visibility rules, known
  limitations (retention_days not enforced; 30s authz cache; single-Center).
- [ ] **6.3** Add a superseded note to the 2026-06-13 spec pointing at the 2026-06-14 design
  (the `access.mode` switch is replaced by the orthogonal axes).
- [ ] **6.4** `cargo test --bin edgion-center config` + `cd web && npm run build` green. Commit:
  `docs: orthogonal access-control config + skills`.

**Acceptance:** an operator can configure any combination from the sample config + skills doc.

---

## Self-review notes

- **Spec coverage:** config axes (T1) ✓; provider assembly + remove access.mode + OIDC-disable +
  placeholder (T3) ✓; unified login DB-then-admin + secret precedence (T2/T3) ✓; rbac
  fail-closed unmapped (T3.5) ✓; startup validation (T3.2) ✓; bootstrap retie (T3) ✓; server-info
  fields (T4) ✓; frontend gating matrix (T5) ✓; docs (T6) ✓; out-of-scope items (default role,
  OIDC interactive flow) absent ✓.
- **Type consistency:** `AuthzMode`/`AuthzConfig`/`DbAuthConfig` (T1) reused in T3/T4;
  `UnifiedLoginState`/`unified_login_handler`/`add_unified_auth_routes` (T2) consumed in T3;
  `authz_mode`+`db_auth_enabled` on `ApiState`/server-info (T4) consumed by the frontend
  `authzMode`+`dbAuthEnabled` (T5). `verify_single_admin`/`issue_login_response` shared.
- **Sequencing risk flagged:** Tasks 1–3 must compile together; commit T1/T2 only if the tree
  builds, else fold into the T3 commit (called out in T1.3 / T2.4).
