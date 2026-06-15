# Center Dual-Tier Access Control — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `edgion-center` two config-selected access-control tiers — `lite` (Okta/OIDC
login, `login=admin`, audit log, SQLite) and `full` (DB local users, page+API RBAC manageable
from the UI, MySQL) — sharing one codebase.

**Architecture:** Replace the `rusqlite` direct DB with a `sqlx`-backed `Store` trait (SQLite +
MySQL). Add an audit middleware (both tiers) and an authz middleware (`AllowAllAuthz` in lite,
`DbAuthz` in full) inside the existing `unified_auth` layer. A code-defined permission-key
catalog tags every business route; the DB stores only role/user/binding rows. The frontend gains
audit, user-management, and role-matrix pages, with menu/button gating from `/auth/me`.

**Tech Stack:** Rust, Axum 0.8, `sqlx` (sqlite + mysql), bcrypt, React + TypeScript + Vite.

**Spec:** `docs/history/superpowers/specs/2026-06-13-center-dual-access-control-design.md`

---

## File Structure

### Backend (`src/`)

- `Cargo.toml` — **modify:** add `sqlx` (features `runtime-tokio`, `sqlite`, `mysql`, `macros`,
  `migrate`), remove `rusqlite`.
- `src/config/mod.rs` — **modify:** extend `DatabaseConfig` (`backend`, `mysql_url`), add
  `AccessConfig`/`AccessMode`, add `AuditConfig`, wire into `CenterConfig`.
- `src/store/` — **new module** (replaces `src/db/`):
  - `mod.rs` — `Store` enum/struct, `StoreBackend`, pool init, `migrate()`.
  - `controllers.rs` — `DbController` + controller CRUD (ports existing behavior).
  - `audit.rs` — `AuditRecord`, `AuditFilter`, insert/list/prune.
  - `users.rs` — `User`/`Role` rows + users/roles/bindings CRUD + lookups (full).
  - `migrations/sqlite/`, `migrations/mysql/` — `sqlx` migration `.sql` files.
- `src/db/` — **delete** after callers migrated.
- `src/common/authz/` — **new module:**
  - `mod.rs` — `Principal`, `PermissionSet`, `Permission` (key newtype), `AuthzStore` trait.
  - `catalog.rs` — the permission-key catalog + `route_permission(method, path) -> Option<Permission>`.
  - `allow_all.rs` — `AllowAllAuthz`.
  - `db_authz.rs` — `DbAuthz` (full; reads from `Store`, short-TTL cache).
  - `middleware.rs` — `authz_middleware` (403 on missing key; pass-through in lite).
- `src/common/audit/` — **new module:**
  - `mod.rs` — `AuditConfig` glue, `AuditSink` (bounded mpsc + background writer task).
  - `middleware.rs` — `audit_middleware` (reads claims, captures status, `source_ip`).
- `src/common/db_auth/` — **new module:** DB-user authn provider for full mode (verify bcrypt,
  issue the same JWT/cookie shape as `local_auth`).
- `src/api/users.rs`, `src/api/roles.rs`, `src/api/audit.rs` — **new:** admin CRUD + read handlers.
- `src/api/mod.rs` — **modify:** annotate routes with permission keys (via `catalog.rs`), mount
  users/roles/audit routes.
- `src/common/api/compose.rs` — **modify:** insert audit + authz layers inside `unified_auth`.
- `src/cli/mod.rs` — **modify:** build `Store` from `backend`, build authz store by `access.mode`,
  build the audit sink, build the DB-auth provider in full mode.

### Frontend (`web/src/`)

- `api/auth.ts` — **modify:** `me()` returns `{ user, permissions: string[] }`.
- `api/audit.ts`, `api/users.ts`, `api/roles.ts` — **new** API clients.
- `pages/Audit/AuditLogPage.tsx` — **new.**
- `pages/Users/UserManagementPage.tsx` — **new.**
- `pages/Roles/RoleManagementPage.tsx` — **new** (permission matrix).
- `utils/permissions.ts` — **new:** permission context/hook (`useCan(key)`).
- `components/shell/menuConfig.tsx` — **modify:** add `requiredPermission` per item; filter.
- `App.tsx` — **modify:** wrap routes in a `PermissionProvider`; add new routes.

---

## Conventions for every task

- TDD: write the failing test, run it red, implement minimally, run it green, commit.
- Rust test command shape: `cargo test -p edgion-center <filter> -- --nocheck` → use
  `cargo test --lib <module>::<test>` (this crate's unit tests live in-module).
- Frontend test command shape: `cd web && npm run test -- <file>` (Vitest) and
  `cd web && npm run build` for type-check.
- Commit after each task with a `feat:`/`refactor:`/`test:` prefix and the
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer.
- MySQL-dependent tests are gated behind an env var `EDGION_TEST_MYSQL_URL`; when unset, those
  tests `eprintln!("skipping: EDGION_TEST_MYSQL_URL unset")` and return. SQLite tests always run
  (in-memory `sqlite::memory:`).

---

## Task 1 — Storage foundation on `sqlx`

**Goal:** Replace `rusqlite`/`src/db` with a `sqlx`-backed `Store` supporting SQLite and MySQL,
preserving `controllers` behavior exactly. No new tables yet beyond `controllers`.

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config/mod.rs` (`DatabaseConfig`)
- Create: `src/store/mod.rs`, `src/store/controllers.rs`,
  `src/store/migrations/sqlite/0001_controllers.sql`,
  `src/store/migrations/mysql/0001_controllers.sql`
- Modify: `src/cli/mod.rs`, `src/api/mod.rs`, `src/fed_sync/server/mod.rs` (callers)
- Delete: `src/db/mod.rs`

**Interfaces:**

```rust
// src/config/mod.rs — extend DatabaseConfig
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DbBackend { Sqlite, Mysql }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    pub enabled: bool,
    pub backend: DbBackend,        // default Sqlite
    pub sqlite_path: String,       // default "data/center.db"
    pub mysql_url: Option<String>, // required when backend = Mysql
}
// Default: enabled=true, backend=Sqlite, sqlite_path="data/center.db", mysql_url=None
```

```rust
// src/store/mod.rs
#[derive(Clone)]
pub struct Store { pool: AnyKind }      // wraps either sqlx::SqlitePool or sqlx::MySqlPool

pub enum AnyKind { Sqlite(sqlx::SqlitePool), Mysql(sqlx::MySqlPool) }

impl Store {
    pub async fn connect(cfg: &DatabaseConfig) -> anyhow::Result<Store>;  // builds pool + runs migrate()
    pub async fn migrate(&self) -> anyhow::Result<()>;                    // sqlx::migrate! per backend
    #[cfg(test)]
    pub async fn open_in_memory() -> anyhow::Result<Store>;               // sqlite::memory:
}
```

```rust
// src/store/controllers.rs — same semantics as old CenterDb, now async
pub struct DbController { pub controller_id: String, pub cluster: String,
    pub env: Vec<String>, pub tag: Vec<String>, pub online: bool, pub last_seen_at: i64 }

impl Store {
    pub async fn upsert_controller(&self, id: &str, cluster: &str, env: &[String], tag: &[String], online: bool) -> anyhow::Result<()>;
    pub async fn mark_offline(&self, id: &str) -> anyhow::Result<()>;
    pub async fn delete_controller(&self, id: &str) -> anyhow::Result<()>;
    pub async fn list_controllers(&self) -> anyhow::Result<Vec<DbController>>;
}
```

Upsert dialect: SQLite uses `ON CONFLICT(controller_id) DO UPDATE`; MySQL uses
`ON DUPLICATE KEY UPDATE`. Branch on `self.pool` inside `upsert_controller`.

**Steps:**

- [ ] **1.1** Add `sqlx` to `Cargo.toml`, remove `rusqlite`. Run `cargo build` (expect errors in
  `src/db` callers — that's fine, we delete it).
- [ ] **1.2** Write migration SQL: `migrations/sqlite/0001_controllers.sql` (the existing
  `controllers` CREATE TABLE + the three legacy `DROP TABLE IF EXISTS`) and the MySQL twin
  (`controller_id VARCHAR(255) PRIMARY KEY`, `online TINYINT`, `*_at BIGINT`, JSON cols as `TEXT`).
- [ ] **1.3** Write the failing test `src/store/controllers.rs::upsert_then_list_roundtrips` (port
  the three existing `src/db/mod.rs` tests verbatim to async + `open_in_memory`).
- [ ] **1.4** Run `cargo test --lib store::controllers` → FAIL (Store not defined).
- [ ] **1.5** Implement `Store::connect`/`migrate`/`open_in_memory` (sqlite branch) and the four
  controller methods (sqlite branch first).
- [ ] **1.6** Run `cargo test --lib store::controllers` → PASS (sqlite).
- [ ] **1.7** Add the MySQL branch to each method + the MySQL-gated round-trip test
  (`EDGION_TEST_MYSQL_URL`). Run with the env var set against a scratch MySQL → PASS; unset → skip.
- [ ] **1.8** Migrate callers (`src/cli/mod.rs` builds `Store::connect`; `src/api/mod.rs` and
  `src/fed_sync/server/mod.rs` call the async methods via the existing async contexts; remove the
  `tokio::task::spawn_blocking` wrappers if present). Delete `src/db/mod.rs` and its `mod db;`.
- [ ] **1.9** Run `cargo build && cargo test --lib` → PASS. Manually start the binary with the
  default SQLite config; register a controller; `GET /api/v1/controllers` returns it.
- [ ] **1.10** Commit: `refactor: replace rusqlite CenterDb with sqlx Store (sqlite+mysql)`.

**Acceptance:** controllers CRUD identical on SQLite; MySQL round-trip passes when configured;
`rusqlite` gone from `Cargo.lock`.

---

## Task 2 — Audit backend (schema + sink + middleware)

**Goal:** Record mutating admin actions with attribution into `audit_log`, fail-open, non-blocking.

**Files:**
- Create: `src/store/audit.rs`, `src/store/migrations/{sqlite,mysql}/0002_audit_log.sql`
- Create: `src/common/audit/mod.rs`, `src/common/audit/middleware.rs`
- Modify: `src/config/mod.rs` (`AuditConfig`), `src/common/api/compose.rs`, `src/cli/mod.rs`

**Interfaces:**

```rust
// src/config/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditConfig { pub enabled: bool, pub log_reads: bool, pub retention_days: u32 }
// Default: enabled=true, log_reads=false, retention_days=0 (unbounded + WARN)
```

```rust
// src/store/audit.rs
pub struct AuditRecord {
    pub ts: i64, pub actor: String, pub provider: String, pub method: String, pub path: String,
    pub target_controller: Option<String>, pub status: i32,
    pub source_ip: Option<String>, pub request_id: Option<String>, pub detail: Option<String>,
}
pub struct AuditFilter { pub actor: Option<String>, pub controller: Option<String>,
    pub since: Option<i64>, pub until: Option<i64> }
impl Store {
    pub async fn insert_audit(&self, rec: &AuditRecord) -> anyhow::Result<()>;
    pub async fn list_audit(&self, f: &AuditFilter, limit: i64, offset: i64) -> anyhow::Result<Vec<AuditRecord>>;
    pub async fn prune_audit(&self, before_ts: i64) -> anyhow::Result<u64>;
}
```

```rust
// src/common/audit/mod.rs
#[derive(Clone)]
pub struct AuditSink { tx: tokio::sync::mpsc::Sender<AuditRecord> }   // bounded (cap 1024)
impl AuditSink {
    pub fn spawn(store: Store) -> AuditSink;       // starts the background drain+insert task
    pub fn record(&self, rec: AuditRecord);        // try_send; on full → drop + metric increment
}
// metric: counter `edgion_center_audit_dropped_total`
```

```rust
// src/common/audit/middleware.rs
// axum from_fn_with_state; reads UnifiedAuthClaims from extensions, ConnectInfo<SocketAddr>
// for source_ip (never X-Forwarded-For), decodes /api/v1/proxy/{id}/... target (`~`→`/`).
pub async fn audit_middleware(/* State<AuditState>, request, next */) -> Response;
```

**Steps:**

- [ ] **2.1** `0002_audit_log.sql` (sqlite + mysql twins) per the spec schema, with
  `idx_audit_log_ts` and `idx_audit_log_actor`.
- [ ] **2.2** Failing test `store::audit::insert_then_list_filters` (insert 3 rows, list with an
  actor filter, assert ordering `ts DESC`).
- [ ] **2.3** Run → FAIL. Implement the three audit `Store` methods. Run → PASS.
- [ ] **2.4** Failing test `common::audit::sink_drops_when_full_and_counts` (cap-1 sink, send 2,
  assert dropped counter == 1). Implement `AuditSink`. Run → PASS.
- [ ] **2.5** Failing test `common::audit::middleware_records_mutation` (axum test app with a fake
  claims extension + POST route; assert one record with correct actor/method/status/source_ip).
- [ ] **2.6** Implement `audit_middleware`; skip `GET` unless `log_reads`; exclude the audit-read
  path. Run → PASS.
- [ ] **2.7** Wire into `compose_admin_routes`: apply `audit_middleware` to `business` **before**
  the `unified_auth` wrap so claims are present (add an optional `audit: Option<AuditState>` arg or
  a builder; layer is inside auth, outside business). Adjust the existing compose tests.
- [ ] **2.8** `src/cli/mod.rs`: build `AuditSink::spawn(store)` when `database.enabled && audit.enabled`.
- [ ] **2.9** `cargo test --lib` PASS; manual: do a `POST .../failover`, then query the DB row exists.
- [ ] **2.10** Commit: `feat: audit log backend (sqlx + non-blocking sink + middleware)`.

**Acceptance:** mutations recorded with correct actor/status/source_ip; reads excluded by default;
full channel drops + increments the metric; request latency unaffected.

---

## Task 3 — Audit read API + frontend audit page

**Files:**
- Create: `src/api/audit.rs`; Modify: `src/api/mod.rs`
- Create: `web/src/api/audit.ts`, `web/src/pages/Audit/AuditLogPage.tsx`;
  Modify: `web/src/App.tsx`, `web/src/components/shell/menuConfig.tsx`

**Interface:** `GET /api/v1/center/admin/audit-logs?limit&offset&actor&controller&since&until`
→ `ListResponse<AuditRecordDto>` (reuse the existing list-response shape). Excluded from `log_reads`.

**Steps:**

- [ ] **3.1** Failing handler test `api::audit::lists_with_filters` (seed store, call handler,
  assert filtered + paginated JSON). Implement `audit_list_handler`. Run → PASS.
- [ ] **3.2** Mount the route in `src/api/mod.rs` business router. `cargo test --lib` PASS.
- [ ] **3.3** `web/src/api/audit.ts`: `auditApi.list(params)` typed against the DTO.
- [ ] **3.4** `AuditLogPage.tsx`: table (ts/actor/method/path/target/status/source_ip) + filter
  inputs + pager. Vitest render test with a mocked client. `cd web && npm run test` PASS.
- [ ] **3.5** Add route `/audit` in `App.tsx` under `RequireAuth`; add a menu item in
  `menuConfig.tsx` (gated by `audit:read`, added in Task 8 — for now always visible).
- [ ] **3.6** `cd web && npm run build` (type-check) PASS.
- [ ] **3.7** Commit: `feat: audit log read API + dashboard page`.

**Acceptance:** dashboard shows the audit trail with working filters/pagination.

---

## Task 4 — Authz abstraction + `[access] mode` (lite tier complete)

**Goal:** Introduce the authz seam. After this task, **lite mode is fully deliverable** (Okta
login + audit + `login=admin`).

**Files:**
- Modify: `src/config/mod.rs` (`AccessConfig`/`AccessMode`)
- Create: `src/common/authz/mod.rs`, `allow_all.rs`, `catalog.rs`, `middleware.rs`
- Modify: `src/common/api/compose.rs`, `src/cli/mod.rs`,
  `src/common/local_auth/handlers.rs` (`/auth/me` payload), `web/src/api/auth.ts`,
  `web/src/utils/permissions.ts` (new), `web/src/App.tsx`

**Interfaces:**

```rust
// src/config/mod.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode { Lite, Full }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessConfig { pub mode: AccessMode }   // default Lite
// add `pub access: AccessConfig` to CenterConfig
```

```rust
// src/common/authz/mod.rs
pub struct Principal { pub subject: String, pub provider: String }
#[derive(Clone, Default)]
pub struct PermissionSet { keys: std::collections::HashSet<String>, all: bool }
impl PermissionSet { pub fn all() -> Self; pub fn contains(&self, key: &str) -> bool; pub fn keys(&self) -> Vec<String>; }

#[async_trait::async_trait]
pub trait AuthzStore: Send + Sync {
    async fn permissions_for(&self, p: &Principal) -> PermissionSet;
}
```

```rust
// src/common/authz/catalog.rs
pub struct Permission(pub &'static str);
// Catalog grouped by page, e.g.:
pub const CONTROLLERS_READ: &str = "controllers:read";
pub const CONTROLLERS_WRITE: &str = "controllers:write";
pub const REGION_ROUTES_READ: &str = "region-routes:read";
pub const REGION_ROUTES_WRITE: &str = "region-routes:write";
pub const IP_RESTRICTIONS_WRITE: &str = "ip-restrictions:write";
pub const AUDIT_READ: &str = "audit:read";
pub const USERS_MANAGE: &str = "users:manage";
pub const ROLES_MANAGE: &str = "roles:manage";
pub fn all_keys() -> &'static [&'static str];
/// Map a request to its required permission key. None => no auth-key requirement (rare).
pub fn route_permission(method: &http::Method, path: &str) -> Option<&'static str>;
```

```rust
// src/common/authz/allow_all.rs
pub struct AllowAllAuthz;
#[async_trait::async_trait] impl AuthzStore for AllowAllAuthz {
    async fn permissions_for(&self, _: &Principal) -> PermissionSet { PermissionSet::all() }
}
// src/common/authz/middleware.rs
pub async fn authz_middleware(/* State<Arc<dyn AuthzStore>>, request, next */) -> Response;
// reads claims → Principal → permissions_for → route_permission(method,path);
// 403 if required key absent; pass-through if PermissionSet.all or no required key.
```

**Steps:**

- [ ] **4.1** Add `AccessMode`/`AccessConfig` + config test (`access.mode: full` parses; default lite).
- [ ] **4.2** Write `catalog.rs` with the key constants + `route_permission` mapping every current
  business route. Failing test `authz::catalog::every_business_route_has_a_key` (table of
  (method,path) → assert `route_permission` is `Some`). Implement until green.
- [ ] **4.3** `PermissionSet` + `AllowAllAuthz` + test `allow_all_contains_everything`.
- [ ] **4.4** `authz_middleware` + tests: `denies_without_key` (DbAuthz stub returning empty → 403),
  `allows_with_all` (AllowAll → 200).
- [ ] **4.5** Wire into `compose_admin_routes`: layer order inside-out = business → audit → authz →
  unified_auth → cache-control. Pass `Arc<dyn AuthzStore>` through. Update compose tests.
- [ ] **4.6** `/auth/me`: extend the JSON to `{ user, provider, permissions: [..] }` using
  `permissions_for(principal)`. Update the handler test.
- [ ] **4.7** `src/cli/mod.rs`: pick `AllowAllAuthz` when `access.mode == Lite` (DbAuthz comes in
  Task 6). `cargo test --lib` PASS.
- [ ] **4.8** Frontend: `auth.ts` `me()` returns permissions; `utils/permissions.ts` exposes
  `PermissionProvider` + `useCan(key)`; `App.tsx` wraps routes. `npm run build` PASS.
- [ ] **4.9** Manual: start in lite mode (default), Okta or local login, all routes reachable,
  audit records actions. Commit: `feat: authz seam + access.mode (lite tier complete)`.

**Acceptance:** lite mode end-to-end (login=admin, audit on); `/auth/me` returns the full key set;
authz middleware denies unknown keys when the store is non-allow-all.

---

## Task 5 — Full-tier schema + permission-key bindings

**Files:**
- Create: `src/store/users.rs`, `src/store/migrations/{sqlite,mysql}/0003_users_roles.sql`

**Schema (0003):** `users`, `roles`, `user_roles`, `role_permissions`, optional `api_tokens`
(per spec §Database schema). MySQL twin uses `VARCHAR`/`BIGINT`/`TINYINT`; SQLite uses `TEXT`/
`INTEGER`. `username`/`name` UNIQUE; composite PKs on the join tables.

**Interfaces:**

```rust
// src/store/users.rs
pub struct User { pub id: i64, pub username: String, pub password_hash: String,
    pub display_name: String, pub status: String, pub created_at: i64, pub updated_at: i64 }
pub struct Role { pub id: i64, pub name: String, pub description: String }
impl Store {
    pub async fn create_user(&self, username: &str, password_hash: &str, display_name: &str) -> anyhow::Result<i64>;
    pub async fn get_user_by_username(&self, username: &str) -> anyhow::Result<Option<User>>;
    pub async fn list_users(&self) -> anyhow::Result<Vec<User>>;
    pub async fn set_user_status(&self, id: i64, status: &str) -> anyhow::Result<()>;
    pub async fn set_user_password(&self, id: i64, password_hash: &str) -> anyhow::Result<()>;
    pub async fn delete_user(&self, id: i64) -> anyhow::Result<()>;
    pub async fn create_role(&self, name: &str, description: &str) -> anyhow::Result<i64>;
    pub async fn list_roles(&self) -> anyhow::Result<Vec<Role>>;
    pub async fn delete_role(&self, id: i64) -> anyhow::Result<()>;
    pub async fn set_role_permissions(&self, role_id: i64, keys: &[String]) -> anyhow::Result<()>;
    pub async fn role_permissions(&self, role_id: i64) -> anyhow::Result<Vec<String>>;
    pub async fn set_user_roles(&self, user_id: i64, role_ids: &[i64]) -> anyhow::Result<()>;
    pub async fn permission_keys_for_user(&self, username: &str) -> anyhow::Result<Vec<String>>; // JOIN
}
```

**Steps:**

- [ ] **5.1** Write `0003` migrations (both backends).
- [ ] **5.2** Failing test `store::users::user_role_permission_join` (create user+role, bind role to
  `controllers:read`, bind user to role, assert `permission_keys_for_user` == `["controllers:read"]`).
- [ ] **5.3** Run → FAIL. Implement the `users.rs` methods (sqlite branch; mysql branch with the
  gated test). Run → PASS (sqlite) and PASS-or-skip (mysql).
- [ ] **5.4** Commit: `feat: full-tier users/roles/bindings schema + store methods`.

**Acceptance:** full join resolves a user's effective permission keys on both backends.

---

## Task 6 — DbAuthz + DB-user authn provider (full login + enforcement)

**Files:**
- Create: `src/common/authz/db_authz.rs`, `src/common/db_auth/mod.rs`,
  `src/common/db_auth/handlers.rs`
- Modify: `src/cli/mod.rs`, `src/common/api/compose.rs` (mount db-auth login in full mode)

**Interfaces:**

```rust
// src/common/authz/db_authz.rs
pub struct DbAuthz { store: Store, cache: moka::future::Cache<String, PermissionSet> } // 30s TTL
#[async_trait::async_trait] impl AuthzStore for DbAuthz {
    async fn permissions_for(&self, p: &Principal) -> PermissionSet { /* cache → store.permission_keys_for_user */ }
}
```

```rust
// src/common/db_auth/handlers.rs — mirror local_auth's login/me/logout shape
// POST /api/v1/auth/login { username, password } → verify bcrypt against users.password_hash,
//   reject if status != "active", issue the same signed JWT + httpOnly cookie as local_auth.
// Reuse local_auth's JWT signing/cookie helpers (extract a shared helper if needed).
```

**Steps:**

- [ ] **6.1** Failing test `db_auth::login_rejects_inactive_user` + `login_ok_issues_cookie`
  (seed user via Store, hit handler). Implement `db_auth` handlers (bcrypt verify, status check,
  JWT issue reusing local_auth helpers). Run → PASS.
- [ ] **6.2** Failing test `authz::db_authz::resolves_user_keys` + `caches`. Implement `DbAuthz`
  (add `moka` dep if not present, else a hand-rolled `RwLock<HashMap>` + Instant TTL). Run → PASS.
- [ ] **6.3** `compose.rs`/`cli`: in full mode, mount db-auth login routes (instead of local_auth)
  and select `DbAuthz` as the `AuthzStore`. Validation: full mode + DB disabled → startup error.
- [ ] **6.4** Middleware end-to-end test: user with only `controllers:read` gets 200 on
  `GET /controllers`, 403 on `POST .../reload`. `cargo test --lib` PASS.
- [ ] **6.5** Manual (sqlite full mode): seed an admin user (bootstrap, see below), login, verify
  enforcement. Commit: `feat: DbAuthz + DB-user login + RBAC enforcement (full tier)`.

**Bootstrap:** on first start in full mode with **zero users**, create an `admin` user from
`EDGION_ADMIN_USERNAME`/`EDGION_ADMIN_PASSWORD` (reuse existing env vars) bound to a built-in
`admin` role holding `all_keys()`; log a WARN if those env vars are unset (no login possible).

**Acceptance:** full-mode login works against DB users; RBAC enforced (403 on missing key);
inactive users rejected; first-run admin bootstrap works.

---

## Task 7 — User/role admin CRUD API

**Files:** Create `src/api/users.rs`, `src/api/roles.rs`; Modify `src/api/mod.rs`.

**Endpoints (all gated `users:manage` / `roles:manage`):**
- `GET/POST /api/v1/center/admin/users`, `PATCH/DELETE /.../users/{id}` (status, password, roles).
- `GET/POST /api/v1/center/admin/roles`, `PUT /.../roles/{id}/permissions`, `DELETE /.../roles/{id}`.
- `GET /api/v1/center/admin/permission-catalog` → the grouped key catalog for the matrix UI.

**Steps:**

- [ ] **7.1** Failing handler tests per endpoint (create user → appears in list; set role perms →
  reflected in `role_permissions`). Implement handlers calling Task 5 `Store` methods. Passwords
  hashed with bcrypt in the create/reset handlers.
- [ ] **7.2** Mount routes (tagged with their permission keys in `catalog.rs`). `cargo test --lib` PASS.
- [ ] **7.3** Commit: `feat: user/role admin CRUD API`.

**Acceptance:** full CRUD over users/roles/bindings; catalog endpoint returns grouped keys.

---

## Task 8 — Frontend: user mgmt + role matrix + menu gating

**Files:** Create `web/src/api/users.ts`, `web/src/api/roles.ts`,
`web/src/pages/Users/UserManagementPage.tsx`, `web/src/pages/Roles/RoleManagementPage.tsx`;
Modify `web/src/components/shell/menuConfig.tsx`, `web/src/App.tsx`.

**Steps:**

- [ ] **8.1** `users.ts`/`roles.ts` API clients typed to Task 7 DTOs.
- [ ] **8.2** `UserManagementPage.tsx`: list/create/disable/delete users, assign roles, reset
  password. Vitest render test with mocked clients.
- [ ] **8.3** `RoleManagementPage.tsx`: role list + permission **matrix** (checkbox grid from
  `permission-catalog`, grouped by page). Save → `PUT roles/{id}/permissions`. Render test.
- [ ] **8.4** `menuConfig.tsx`: add `requiredPermission` to each item; filter the menu with
  `useCan`. Users/Roles items gated by `users:manage`/`roles:manage`; Audit by `audit:read`.
- [ ] **8.5** Add `/users` and `/roles` routes (gated) in `App.tsx`. `npm run test && npm run build` PASS.
- [ ] **8.6** Commit: `feat: user-management + role-matrix dashboard pages + menu gating`.

**Acceptance:** full-mode admin manages users/roles/permissions from the UI; lite-mode users see
everything (all keys); non-admins don't see management menus and get 403 if they force the API.

---

## Task 9 — Wrap-up: configs, docs, migration notes

**Files:** `config/edgion-center.yaml` (examples), `skills/02-features/` (mode docs),
`docs/history/tasks/center-auth-rbac-design.md` + `center-audit-log.md` (mark superseded).

**Steps:**

- [ ] **9.1** Add commented example blocks for both modes (`access.mode`, `database.backend`,
  `audit`) to the sample config.
- [ ] **9.2** Write `skills/02-features/access-control.md` (lite vs full, config keys, permission
  catalog, bootstrap) and link it from `skills/02-features/SKILL.md`.
- [ ] **9.3** Mark the two old task drafts `Status: superseded by 2026-06-13 dual-access plan`.
- [ ] **9.4** Final `cargo test --lib && cd web && npm run build`. Commit: `docs: access-control
  config examples + skills`.

**Acceptance:** an operator can configure either tier from the sample config + skills doc alone.

---

## Self-review notes

- **Spec coverage:** mode switch (T4) ✓; sqlx storage + delete rusqlite (T1) ✓; audit (T2/T3) ✓;
  page+API RBAC catalog (T4/T5/T7) ✓; DB users + bcrypt (T6) ✓; frontend pages + gating (T3/T8) ✓;
  error handling — full+no-DB startup error (T6.3), missing-key 403 (T4.4), route-coverage check
  (T4.2), fail-open audit (T2.4) ✓; out-of-scope items absent ✓.
- **Type consistency:** `Store`, `AuthzStore`, `PermissionSet`, `Principal`, `AuditRecord`,
  `permission_keys_for_user`, `route_permission` are defined once (T1/T2/T4/T5) and reused with the
  same signatures downstream.
- **Open risk flagged for execution:** `moka` vs hand-rolled cache (T6.2) — pick hand-rolled if
  adding a dep is undesirable; behavior identical.
