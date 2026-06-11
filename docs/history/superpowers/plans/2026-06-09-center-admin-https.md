# Center Admin API HTTPS Support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the Center Admin API serve over HTTPS when an optional `admin_tls` config section is present, falling back to today's plain-HTTP path with full backward compatibility.

**Architecture:** Add a server-only `AdminTlsConfig { cert, key }` in `common` config, wire an optional `admin_tls` field into `CenterServerConfig`, and branch the Center admin serve path between `axum::serve` (HTTP) and `axum_server::bind_rustls` (HTTPS). Add startup diagnostics (validate + a pure cookie/TLS warning function). No mTLS, no probe/metrics TLS, no runtime behavior change to cookies.

**Tech Stack:** Rust, axum 0.8, axum-server 0.8 (`tls-rustls`), rustls 0.23 (ring), serde_yaml, rcgen + tempfile (tests).

**Spec:** `docs/superpowers/specs/2026-06-09-center-admin-https-design.md`

**Project rule:** Per `CLAUDE.md`, do **not** run `git commit` autonomously — each task ends with a commit step, but get the user's go-ahead before running it. All on-disk text must be English.

---

## File Structure

- **`src/core/common/config/mod.rs`** — new `pub struct AdminTlsConfig` (fields, custom `Debug`, `validate()`, `cert_path()`/`key_path()`), mirroring the existing `ConfSyncTlsConfig` (`:474-536`). Unit tests in the existing `#[cfg(test)] mod tests` (`:952`).
- **`src/core/center/config/mod.rs`** — add `admin_tls: Option<AdminTlsConfig>` to `CenterServerConfig` (`:83-105`), update its manual `Default`, extend the common-config `use` (`:2`). Add YAML deser tests in the existing test module.
- **`src/core/center/cli/mod.rs`** — pure `admin_tls_cookie_warning()` fn + its startup call; clone `admin_tls` before the `http_handle` spawn; sync `validate()` call in `serve()`; serve-path HTTP/HTTPS branch; change the spawn closure to return `anyhow::Result<()>`. Tests (warning predicate + TLS PEM load happy/negative) in the existing `#[cfg(test)] mod tests` (`:300`).

No new files. No new dependencies (all present in `Cargo.toml`).

---

## Task 1: `AdminTlsConfig` struct in common config

**Files:**
- Modify: `src/core/common/config/mod.rs` (add struct near `ConfSyncTlsConfig` at `:474-536`)
- Test: `src/core/common/config/mod.rs` (existing `#[cfg(test)] mod tests` at `:952`)

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block (around `:953`, after the `use super::*;` line — confirm that import exists; if it does not, add `use super::AdminTlsConfig;`):

```rust
    #[test]
    fn test_admin_tls_validate() {
        let ok = AdminTlsConfig { cert: "c".into(), key: "k".into() };
        assert!(ok.validate().is_ok());

        let no_cert = AdminTlsConfig { cert: "   ".into(), key: "k".into() };
        assert!(no_cert.validate().is_err());

        let no_key = AdminTlsConfig { cert: "c".into(), key: "".into() };
        assert!(no_key.validate().is_err());
    }

    #[test]
    fn test_admin_tls_debug_redacts_key() {
        let c = AdminTlsConfig {
            cert: "certs/admin/server.crt".into(),
            key: "certs/admin/private.key".into(),
        };
        let dbg = format!("{c:?}");
        assert!(dbg.contains("***"), "key must be redacted: {dbg}");
        assert!(!dbg.contains("private.key"), "key path leaked: {dbg}");
        assert!(dbg.contains("server.crt"), "cert path should be visible: {dbg}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p edgion --lib core::common::config::tests::test_admin_tls -- --nocapture`
Expected: FAIL — `cannot find type AdminTlsConfig in this scope` (struct not defined yet).

- [ ] **Step 3: Write the struct**

Insert immediately after the `ConfSyncTlsConfig` block (after its `impl` ends at `:536`) in `src/core/common/config/mod.rs`. `PathBuf` is already imported in this file (used by `ConfSyncTlsConfig`):

```rust
/// Server-side TLS certificate file paths for the admin HTTP API.
///
/// Unlike `ConfSyncTlsConfig` (mTLS — requires `ca`), this is server-only TLS:
/// only `cert` + `key` are needed, with no client-certificate verification. Lives
/// in `common` so the Controller can reuse it later (see admin-api-03).
#[derive(Clone, Serialize, Deserialize)]
pub struct AdminTlsConfig {
    /// PEM server certificate path (relative to work_dir or absolute).
    pub cert: String,
    /// PEM private key path (relative to work_dir or absolute).
    pub key: String,
}

impl std::fmt::Debug for AdminTlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminTlsConfig")
            .field("cert", &self.cert)
            .field("key", &"***")
            .finish()
    }
}

impl AdminTlsConfig {
    /// Validate that the cert and key paths are non-empty.
    /// Called before attempting to load certificates at startup.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.cert.trim().is_empty() {
            anyhow::bail!("admin_tls.cert must not be empty");
        }
        if self.key.trim().is_empty() {
            anyhow::bail!("admin_tls.key must not be empty");
        }
        Ok(())
    }

    /// Resolve the cert path: absolute as-is, relative joined with work_dir.
    pub fn cert_path(&self) -> PathBuf {
        crate::types::work_dir().resolve(&self.cert)
    }

    /// Resolve the key path: absolute as-is, relative joined with work_dir.
    pub fn key_path(&self) -> PathBuf {
        crate::types::work_dir().resolve(&self.key)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p edgion --lib core::common::config::tests::test_admin_tls -- --nocapture`
Expected: PASS (both `test_admin_tls_validate` and `test_admin_tls_debug_redacts_key`).

- [ ] **Step 5: Commit**

```bash
git add src/core/common/config/mod.rs
git commit -m "feat(center): add AdminTlsConfig (server-only TLS cert/key) in common config"
```

---

## Task 2: Wire `admin_tls` into `CenterServerConfig`

**Files:**
- Modify: `src/core/center/config/mod.rs` (`use` at `:2`, struct `:83-94`, `Default` `:96-105`)
- Test: `src/core/center/config/mod.rs` (existing test module)

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/core/center/config/mod.rs` (alongside `test_center_config_defaults`):

```rust
    #[test]
    fn test_admin_tls_deserializes_from_yaml() {
        let yaml = r#"
server:
  http_addr: "0.0.0.0:5900"
  admin_tls:
    cert: "certs/admin/server.crt"
    key: "certs/admin/server.key"
"#;
        let cfg: CenterConfig = serde_yaml::from_str(yaml).unwrap();
        let tls = cfg.server.admin_tls.expect("admin_tls should parse");
        assert_eq!(tls.cert, "certs/admin/server.crt");
        assert_eq!(tls.key, "certs/admin/server.key");
    }

    #[test]
    fn test_admin_tls_absent_is_none() {
        let yaml = r#"
server:
  http_addr: "0.0.0.0:5900"
"#;
        let cfg: CenterConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.server.admin_tls.is_none(), "omitted admin_tls must be None");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p edgion --lib core::center::config::tests::test_admin_tls -- --nocapture`
Expected: FAIL — `no field admin_tls on type CenterServerConfig`.

- [ ] **Step 3: Add the field, the import, and update `Default`**

In `src/core/center/config/mod.rs`:

Extend the common-config import at `:2`:

```rust
use crate::core::common::config::{AdminTlsConfig, ConfSyncSecurityConfig};
```

Add the field to `CenterServerConfig` (after `metrics_addr`, around `:93`):

```rust
    /// Optional server-side TLS for the admin HTTP API. Omit -> plain HTTP.
    /// When set, the admin listener serves HTTPS; probe/metrics stay HTTP.
    pub admin_tls: Option<AdminTlsConfig>,
```

Update the manual `Default` impl (around `:96-105`) to initialize the new field:

```rust
impl Default for CenterServerConfig {
    fn default() -> Self {
        Self {
            grpc_addr: "0.0.0.0:12251".to_string(),
            http_addr: "0.0.0.0:12201".to_string(),
            probe_addr: "0.0.0.0:12200".to_string(),
            metrics_addr: "0.0.0.0:12290".to_string(),
            admin_tls: None,
        }
    }
}
```

(`CenterServerConfig` already has struct-level `#[serde(default)]`, so the new `Option` field needs no field-level attribute; an omitted `admin_tls` deserializes to `None`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p edgion --lib core::center::config::tests::test_admin_tls -- --nocapture`
Expected: PASS (both deser tests).

- [ ] **Step 5: Commit**

```bash
git add src/core/center/config/mod.rs
git commit -m "feat(center): add optional server.admin_tls field to CenterServerConfig"
```

---

## Task 3: Pure `cookie_secure` ↔ TLS startup warning

**Files:**
- Modify: `src/core/center/cli/mod.rs` (add free fn near the top-level fns; extend test `use` at `:302`)
- Test: `src/core/center/cli/mod.rs` (existing `#[cfg(test)] mod tests` at `:300`)

- [ ] **Step 1: Write the failing test**

In `src/core/center/cli/mod.rs`, extend the test-module import at `:302` to include the new fn:

```rust
    use super::{admin_tls_cookie_warning, decide_transport, TransportDecision};
```

Add this test inside `mod tests`:

```rust
    #[test]
    fn test_admin_tls_cookie_warning_combinations() {
        // Contradiction: Center terminates HTTPS but cookie is non-Secure -> warn.
        assert!(admin_tls_cookie_warning(true, false).is_some());
        // Silent-drop risk: Secure cookie but admin listener is plain HTTP -> warn.
        assert!(admin_tls_cookie_warning(false, true).is_some());
        // Safe: HTTPS + Secure cookie.
        assert!(admin_tls_cookie_warning(true, true).is_none());
        // Safe: plain HTTP + non-Secure cookie (consistent dev setup).
        assert!(admin_tls_cookie_warning(false, false).is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p edgion --lib core::center::cli::tests::test_admin_tls_cookie_warning -- --nocapture`
Expected: FAIL — `cannot find function admin_tls_cookie_warning`.

- [ ] **Step 3: Write the pure function**

Add as a free function in `src/core/center/cli/mod.rs` (place it just above the `impl`/`serve` block, at module scope so the test can import it):

```rust
/// Decide which admin-TLS / cookie_secure startup warning to emit, if any.
///
/// Pure (no logging) so it is unit-testable without a tracing capture layer.
/// Returns `None` when the combination is safe. Only meaningful when local auth
/// is configured (otherwise no cookie is ever issued — the caller gates on that).
fn admin_tls_cookie_warning(admin_tls_present: bool, cookie_secure: bool) -> Option<&'static str> {
    match (admin_tls_present, cookie_secure) {
        // Center itself terminates HTTPS but the auth cookie is non-Secure.
        (true, false) => Some(
            "admin_tls is enabled but local_auth.cookie_secure=false: the auth cookie will be \
             issued without the Secure attribute over HTTPS. Set cookie_secure=true.",
        ),
        // Secure cookie but plain-HTTP listener: a direct http:// browser reach silently
        // drops the session cookie. Fine behind a TLS-terminating proxy (warn-only).
        (false, true) => Some(
            "local_auth.cookie_secure=true but admin_tls is not configured: if Center is reached \
             directly over http:// the browser will drop the session cookie. Ensure a \
             TLS-terminating proxy fronts the admin listener.",
        ),
        _ => None,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p edgion --lib core::center::cli::tests::test_admin_tls_cookie_warning -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/core/center/cli/mod.rs
git commit -m "feat(center): add pure admin_tls/cookie_secure startup warning predicate"
```

---

## Task 4: TLS PEM load tests (happy + negative)

These validate the `axum_server` dependency and the "must not panic on bad cert" requirement **before** wiring the serve branch. Note `RustlsConfig::from_pem_file` builds a rustls `ServerConfig`, which needs a process-level `CryptoProvider` — installed in the binary `main` but **not** in unit tests, so the happy-path test installs ring explicitly.

**Files:**
- Test: `src/core/center/cli/mod.rs` (existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the tests**

Add inside `mod tests` in `src/core/center/cli/mod.rs`:

```rust
    #[tokio::test]
    async fn test_rustls_config_loads_self_signed_pem() {
        use rcgen::{CertificateParams, KeyPair};
        // ServerConfig needs a process CryptoProvider; main installs it, tests must too.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let key_pair = KeyPair::generate().expect("rcgen key generation failed");
        let params =
            CertificateParams::new(vec!["localhost".to_string()]).expect("rcgen params failed");
        let cert = params.self_signed(&key_pair).expect("rcgen self-sign failed");

        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("server.crt");
        let key_path = dir.path().join("server.key");
        std::fs::write(&cert_path, cert.pem()).unwrap();
        std::fs::write(&key_path, key_pair.serialize_pem()).unwrap();

        let res =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path).await;
        assert!(res.is_ok(), "valid self-signed PEM should load: {:?}", res.err());
    }

    #[tokio::test]
    async fn test_rustls_config_rejects_malformed_pem() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("bad.crt");
        let key_path = dir.path().join("bad.key");
        std::fs::write(&cert_path, b"not a pem at all").unwrap();
        std::fs::write(&key_path, b"also not a key").unwrap();

        let res =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path).await;
        assert!(res.is_err(), "malformed PEM must return Err, not panic");
    }

    #[tokio::test]
    async fn test_rustls_config_rejects_missing_path() {
        let res = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            "/no/such/dir/cert.crt",
            "/no/such/dir/key.key",
        )
        .await;
        assert!(res.is_err(), "missing cert/key files must return Err, not panic");
    }
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p edgion --lib core::center::cli::tests::test_rustls_config -- --nocapture`
Expected: PASS for all three. (These exercise the dependency directly; the serve wiring lands in Task 5. If the happy-path test fails with a `CryptoProvider` panic, confirm the `install_default()` line is present.)

- [ ] **Step 3: Commit**

```bash
git add src/core/center/cli/mod.rs
git commit -m "test(center): cover RustlsConfig PEM load (happy + malformed + missing)"
```

---

## Task 5: Serve-path HTTP/HTTPS branch + validate + warning call

This is the integration step: clone `admin_tls`, validate it synchronously, emit the startup warning, and branch the `http_handle` task between HTTP and HTTPS with a `Result`-returning closure. There is no automated test here (end-to-end listener startup is integration-test territory); correctness rests on Tasks 1–4 plus compile + a manual smoke check.

**Files:**
- Modify: `src/core/center/cli/mod.rs` — validate (~`:126`/`:169`), clone + warning (~`:208-211`), serve branch (`:248-254`)

- [ ] **Step 1: Add the synchronous `admin_tls` validation in `serve()`**

In `src/core/center/cli/mod.rs`, after the `http_addr`/`grpc_addr`/... parse block (around `:169-172`), add:

```rust
        // Validate admin TLS paths up front (cheap, sync) before spawning listeners,
        // mirroring the grpc_security.validate()? gate above.
        if let Some(tls) = config.server.admin_tls.as_ref() {
            tls.validate()
                .map_err(|e| anyhow::anyhow!("Invalid admin_tls config: {}", e))?;
        }
```

- [ ] **Step 2: Clone `admin_tls` and emit the startup warning before the `http_handle` spawn**

Near the existing clones (around `:208-211`, where `auth_config` / `local_auth_config` are cloned), add:

```rust
        let admin_tls = config.server.admin_tls.clone();
        // Startup diagnostic only — does not change cookie runtime behavior.
        if let Some(local) = config.local_auth.as_ref() {
            if let Some(msg) = admin_tls_cookie_warning(admin_tls.is_some(), local.cookie_secure) {
                tracing::warn!(component = "center", "{}", msg);
            }
        }
```

- [ ] **Step 3: Rewrite the `http_handle` serve tail to branch and return `Result`**

Replace the current tail of the `http_handle` closure (`:248-254`):

```rust
            let base_router = router(api_state);
            let app = crate::core::common::api::compose_admin_routes(base_router, auth_state, local_auth_intent);
            let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
```

with:

```rust
            let base_router = router(api_state);
            let app = crate::core::common::api::compose_admin_routes(base_router, auth_state, local_auth_intent);
            // Shared make-service with connect-info: forward-compatible with admin-api-02's
            // IP allowlist (which needs ConnectInfo<SocketAddr>); harmless without it.
            let make_service =
                app.into_make_service_with_connect_info::<std::net::SocketAddr>();
            match admin_tls {
                Some(tls) => {
                    let cert_path = tls.cert_path();
                    let key_path = tls.key_path();
                    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
                        &cert_path,
                        &key_path,
                    )
                    .await
                    .map_err(|e| {
                        tracing::error!(component = "center", error = %e, "Failed to load admin TLS cert/key");
                        anyhow::anyhow!("admin TLS load failed: {}", e)
                    })?;
                    tracing::info!(component = "center", addr = %http_addr, "Center admin API HTTPS enabled");
                    axum_server::bind_rustls(http_addr, rustls_config)
                        .serve(make_service)
                        .await
                        .map_err(|e| anyhow::anyhow!("admin HTTPS server error: {}", e))?;
                }
                None => {
                    let listener = tokio::net::TcpListener::bind(http_addr)
                        .await
                        .map_err(|e| anyhow::anyhow!("admin HTTP bind error: {}", e))?;
                    tracing::info!(component = "center", addr = %http_addr, "Center admin API HTTP enabled (no TLS)");
                    axum::serve(listener, make_service)
                        .await
                        .map_err(|e| anyhow::anyhow!("admin HTTP server error: {}", e))?;
                }
            }
            Ok::<(), anyhow::Error>(())
```

Because the closure body now ends in `Ok::<(), anyhow::Error>(())` and uses `?`, the `tokio::spawn` produces a `JoinHandle<anyhow::Result<()>>`. The `tokio::select!` arm `r = http_handle => { tracing::error!(... "{:?}", r); ... }` (`:292-294`) needs **no change** — `r` is now `Result<anyhow::Result<()>, JoinError>`, still `Debug`, and the `{:?}` prints the anyhow cause chain.

- [ ] **Step 4: Compile**

Run: `cargo check -p edgion --all-targets`
Expected: builds clean. If the borrow checker complains that `make_service` is moved twice, confirm it is consumed inside the two **mutually exclusive** match arms (allowed) and not referenced after the `match`.

- [ ] **Step 5: Run the full Center test module + the new tests**

Run: `cargo test -p edgion --lib core::center -- --nocapture`
Expected: PASS, including `center_no_auth_config_admin_routes_return_503` (the existing fail-close regression — confirms the HTTP branch still wires the same `app`).

- [ ] **Step 6: Manual smoke check (optional but recommended)**

Generate a throwaway cert, point a minimal Center YAML at it, and confirm HTTPS serves while omitting `admin_tls` still serves HTTP. Example:

```bash
# In a scratch dir:
openssl req -x509 -newkey rsa:2048 -nodes -keyout k.key -out c.crt -days 1 -subj "/CN=localhost"
# With admin_tls.cert=c.crt, admin_tls.key=k.key in the Center config:
curl -vk https://127.0.0.1:12201/health    # TLS handshake succeeds
# Without admin_tls in the config:
curl -v  http://127.0.0.1:12201/health     # plain HTTP still works
```

Expected: HTTPS handshake succeeds with `admin_tls` set; plain HTTP works when omitted.

- [ ] **Step 7: Commit**

```bash
git add src/core/center/cli/mod.rs
git commit -m "feat(center): serve admin API over HTTPS when admin_tls is configured"
```

---

## Task 6: Full pre-commit check + docs consistency

**Files:** none (verification only)

- [ ] **Step 1: Format**

Run: `cargo fmt --all`
Expected: no diff, or formatting applied to the touched files.

- [ ] **Step 2: Compile check**

Run: `cargo check --all-targets`
Expected: clean.

- [ ] **Step 3: Lint**

Run: `cargo clippy --all-targets`
Expected: no new warnings on the touched files.

- [ ] **Step 4: Doc consistency**

Run: `make check-agent-docs`
Expected: pass. (No skills/docs were changed; this confirms nothing drifted.)

- [ ] **Step 5: Targeted test sweep**

Run: `cargo test -p edgion --lib core::common::config::tests::test_admin_tls core::center -- --nocapture`
Expected: all admin-TLS + Center tests PASS.

- [ ] **Step 6: Commit any formatting-only changes**

```bash
git add -A
git commit -m "chore(center): fmt/clippy cleanup for admin_tls HTTPS support"
```

(Skip if Steps 1–5 produced no changes.)

---

## Done-When

- `server.admin_tls: { cert, key }` in Center YAML makes the admin API serve HTTPS; omitting it serves HTTP exactly as before.
- Empty `cert`/`key` fails fast at startup with a clear error; malformed/missing cert files produce a logged `Err` and process exit (fail-stop), not a panic.
- A non-`Secure` cookie under HTTPS, or a `Secure` cookie under plain HTTP, emits a startup WARN; safe combinations stay silent.
- Probe (`:12200`) and metrics (`:12290`) listeners are untouched (still HTTP).
- `cargo fmt --all --check`, `cargo check --all-targets`, `cargo clippy --all-targets`, and `make check-agent-docs` all pass.

## Out of Scope (from spec — do NOT implement here)

- mTLS / client-cert (`ca`); Controller HTTPS (task 03); probe/metrics TLS; runtime cert hot-reload; edgion-cli HTTPS client wiring; deployment manifests/Helm for default-secure. See spec "Out of Scope" and "Client-side impact".
