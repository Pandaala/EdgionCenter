# Center L4 TCP IP Pre-Filter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop unauthorized TCP connections on Center's HTTPS admin listener *before* the rustls handshake, via a custom `axum-server` acceptor driven by a dedicated `allow_tcp_ips` allowlist.

**Architecture:** A self-contained `ip_acceptor` module builds an `IpRadixMatcher` from `allow_tcp_ips` and exposes `TcpIpAcceptor`, which implements `axum_server::accept::Accept<TcpStream, S>`. Composed as `RustlsAcceptor::new(cfg).acceptor(TcpIpAcceptor::new(m))`, its IP check runs before TLS; returning `Err` drops the connection pre-handshake. Independent from the L7 `ip_allowlist` middleware — separate config field, module, counter; only the lower-level `core::common::matcher` library is shared.

**Tech Stack:** Rust, `axum-server` 0.8, `tokio`, the `metrics` crate facade, `core::common::matcher` (IP radix tree + CIDR helpers).

**Spec:** `docs/superpowers/specs/2026-06-10-center-l4-tcp-ip-acceptor-design.md`

**Note on commits:** `docs/superpowers/` is gitignored, so the spec/plan are local-only. Commit steps below stage **source files only**. Per project policy, only commit when the user has authorized it for this execution.

---

## File Structure

- **Create** `src/core/common/api/ip_acceptor.rs` — the entire L4 feature: `validate_tcp_ips`, `build_tcp_ip_matcher`, `tcp_ip_allowed`, `TcpIpAcceptor` + `Accept` impl, `record_denied`, and tests. Self-contained; does not import `ip_allowlist`.
- **Modify** `src/core/common/api/mod.rs` — declare `pub mod ip_acceptor;`.
- **Modify** `src/core/center/config/mod.rs` — add the `allow_tcp_ips` field + its `Default` initializer.
- **Modify** `src/core/center/cli/mod.rs` — startup validation, set-but-no-TLS WARN, pre-spawn clone, and the HTTPS-branch acceptor split.

---

## Task 1: `ip_acceptor` module — validation, matcher build, pure predicate

**Files:**
- Create: `src/core/common/api/ip_acceptor.rs`
- Modify: `src/core/common/api/mod.rs:2`

This task lands the module skeleton (no acceptor yet) plus the pure, easily-unit-tested logic. Mirrors the existing `ip_allowlist.rs` infra but is fully independent.

- [ ] **Step 1: Declare the module**

In `src/core/common/api/mod.rs`, add directly below the existing `pub mod ip_allowlist;` (line 2):

```rust
pub mod ip_acceptor;
```

- [ ] **Step 2: Create the module with infra + pure predicate + failing tests**

Create `src/core/common/api/ip_acceptor.rs`:

```rust
//! Admin API L4 TCP IP pre-filter (fr admin-api-04).
//!
//! Drops TCP connections whose kernel peer IP is not in the operator-configured
//! `allow_tcp_ips` CIDR allowlist, BEFORE the rustls handshake, on Center's HTTPS
//! admin listener. Implemented as a custom `axum_server::accept::Accept` placed
//! before `RustlsAcceptor`: returning `Err` aborts the connection pre-handshake.
//!
//! Independent from the L7 `ip_allowlist` middleware — separate config field,
//! module, and denial counter. Only the lower-level `core::common::matcher`
//! library is shared.
//!
//! Precondition: the decision input is `TcpStream::peer_addr()` (the kernel TCP
//! peer). The feature is only correct when that is the real client — behind an
//! SNAT load balancer / terminating proxy it either no-ops or drops everything.
//! See the design spec for the full topology requirements.

use std::io;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use metrics::counter;
use tokio::net::TcpStream;

use crate::core::common::matcher::ip_cidr_helpers::{to_cidr_format, validate_ip_or_cidr};
use crate::core::common::matcher::ip_radix_tree::IpRadixMatcher;

/// Single group label for the flat L4 allowlist matcher.
const TCP_ALLOW_GROUP: &str = "tcp-allow";

/// Metric name kept as a const to match the codebase's name-as-const discipline
/// (every `counter!` call site uses a const, never a string literal). Prefix is
/// `edgion_l4_` (an admin-listener L4 metric), not `edgion_fed_`.
const DENIED_TOTAL: &str = "edgion_l4_tcp_denied_total";

/// Validate every CIDR entry (fail-fast at config load). Reuses
/// `validate_ip_or_cidr`; does not build a matcher.
pub fn validate_tcp_ips(cidrs: &[String]) -> anyhow::Result<()> {
    for cidr in cidrs {
        validate_ip_or_cidr(cidr).map_err(|e| anyhow::anyhow!("invalid allow_tcp_ips entry: {}", e))?;
    }
    Ok(())
}

/// Build the L4 allowlist matcher. `Ok(None)` when empty (allow-all: caller must
/// not mount the acceptor). `Err` on a malformed CIDR (defense in depth).
pub fn build_tcp_ip_matcher(cidrs: &[String]) -> anyhow::Result<Option<Arc<IpRadixMatcher>>> {
    if cidrs.is_empty() {
        return Ok(None);
    }
    let mut builder = IpRadixMatcher::builder();
    for cidr in cidrs {
        let norm =
            to_cidr_format(cidr).map_err(|e| anyhow::anyhow!("invalid allow_tcp_ips entry '{}': {}", cidr, e))?;
        builder
            .insert(&norm, TCP_ALLOW_GROUP)
            .map_err(|e| anyhow::anyhow!("invalid allow_tcp_ips entry '{}': {}", cidr, e))?;
    }
    let matcher = builder
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build allow_tcp_ips matcher: {}", e))?;
    Ok(Some(Arc::new(matcher)))
}

/// Pure decision predicate: the (canonicalized) peer IP matches the allowlist.
/// IPv4-mapped IPv6 (`::ffff:a.b.c.d`) is unmapped via `to_canonical()` first,
/// matching the L7 middleware's behavior.
fn tcp_ip_allowed(matcher: &IpRadixMatcher, ip: IpAddr) -> bool {
    matcher.matched_group(&ip.to_canonical()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(cidrs: &[&str]) -> Arc<IpRadixMatcher> {
        let v: Vec<String> = cidrs.iter().map(|s| s.to_string()).collect();
        build_tcp_ip_matcher(&v).unwrap().unwrap()
    }

    #[test]
    fn ipv4_in_cidr_allowed() {
        let m = matcher(&["10.0.0.0/8"]);
        assert!(tcp_ip_allowed(&m, "10.1.2.3".parse().unwrap()));
    }

    #[test]
    fn ipv4_outside_cidr_denied() {
        let m = matcher(&["10.0.0.0/8"]);
        assert!(!tcp_ip_allowed(&m, "192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn ipv6_in_cidr_allowed() {
        let m = matcher(&["2001:db8::/32"]);
        assert!(tcp_ip_allowed(&m, "2001:db8::1".parse().unwrap()));
    }

    /// Key regression: a dual-stack listener reports an IPv4 client as
    /// `::ffff:10.1.2.3`; it must match an IPv4 CIDR after to_canonical().
    #[test]
    fn ipv4_mapped_ipv6_matches_ipv4_cidr() {
        let m = matcher(&["10.0.0.0/8"]);
        assert!(tcp_ip_allowed(&m, "::ffff:10.1.2.3".parse().unwrap()));
    }

    #[test]
    fn empty_allowlist_returns_none() {
        assert!(build_tcp_ip_matcher(&[]).unwrap().is_none());
    }

    #[test]
    fn malformed_cidr_errors() {
        assert!(build_tcp_ip_matcher(&["10.0.0/8".to_string()]).is_err());
        assert!(validate_tcp_ips(&["10.0.0/8".to_string()]).is_err());
        assert!(validate_tcp_ips(&["10.0.0.0/8".to_string(), "::1".to_string()]).is_ok());
    }

    #[test]
    fn metric_name_is_stable() {
        assert_eq!(DENIED_TOTAL, "edgion_l4_tcp_denied_total");
    }
}
```

Note: `TCP_ALLOW_GROUP`, `DENIED_TOTAL`, the `counter`/`TcpStream`/`io`/`AtomicU64`/`Ordering` imports, and `record_denied` are referenced by Task 2; until then `cargo build` warns about unused items. The `#[allow(dead_code)]` is **not** needed because the test module + Task 2 follow immediately. If you run `cargo build` between tasks and want a clean build, complete Task 2 before building the whole crate; `cargo test` on this module alone passes now.

- [ ] **Step 3: Run the unit tests — verify they pass**

Run: `cargo test -p edgion --lib core::common::api::ip_acceptor`
(If the crate name differs, use `cargo test --lib ip_acceptor`.)
Expected: 7 tests pass (`ipv4_in_cidr_allowed`, `ipv4_outside_cidr_denied`, `ipv6_in_cidr_allowed`, `ipv4_mapped_ipv6_matches_ipv4_cidr`, `empty_allowlist_returns_none`, `malformed_cidr_errors`, `metric_name_is_stable`).

- [ ] **Step 4: Commit (source only)**

```bash
git add src/core/common/api/mod.rs src/core/common/api/ip_acceptor.rs
git commit -m "feat(center): add L4 ip_acceptor matcher infra (admin-api-04)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: `TcpIpAcceptor` + `Accept` impl + denial recording + real-socket test

**Files:**
- Modify: `src/core/common/api/ip_acceptor.rs`

- [ ] **Step 1: Add the acceptor, constructor, and `record_denied`**

Insert into `src/core/common/api/ip_acceptor.rs`, after the `tcp_ip_allowed` function (before the `#[cfg(test)]` module):

```rust
/// Custom acceptor placed BEFORE `RustlsAcceptor`. Runs the IP check on the raw
/// `TcpStream`; on a non-allowlisted peer it returns `Err`, which `axum-server`
/// turns into a silent connection drop *before* the TLS handshake.
///
/// Must be `Clone + Send + Sync + 'static` — `axum-server`'s `serve` requires it,
/// and `RustlsAcceptor`'s `Clone` is conditional on the inner acceptor's.
#[derive(Clone)]
pub struct TcpIpAcceptor {
    matcher: Arc<IpRadixMatcher>,
}

impl TcpIpAcceptor {
    pub fn new(matcher: Arc<IpRadixMatcher>) -> Self {
        Self { matcher }
    }
}

impl<S> axum_server::accept::Accept<TcpStream, S> for TcpIpAcceptor {
    type Stream = TcpStream;
    type Service = S;
    type Future = std::future::Ready<io::Result<(TcpStream, S)>>;

    // Synchronous (returns a `Ready` future), mirroring axum-server's own
    // `NoDelayAcceptor`. Keep it non-blocking: `RustlsAcceptor::handshake_timeout`
    // wraps only the TLS future, NOT this inner acceptor, so any async work added
    // here would be unprotected by a timeout.
    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        match stream.peer_addr() {
            Ok(addr) if tcp_ip_allowed(&self.matcher, addr.ip()) => std::future::ready(Ok((stream, service))),
            other => {
                record_denied(other.ok().map(|a| a.ip()));
                std::future::ready(Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "tcp ip not allowed",
                )))
            }
        }
    }
}

/// Record a denied connection: always bump the Prometheus counter; rate-limit the
/// WARN log (1st, then every 256th) to bound volume under a flood. Independent
/// from the L7 allowlist's counter/log.
fn record_denied(peer: Option<IpAddr>) {
    counter!(DENIED_TOTAL).increment(1);
    static DENIED: AtomicU64 = AtomicU64::new(0);
    let n = DENIED.fetch_add(1, Ordering::Relaxed);
    if n == 0 || n % 256 == 0 {
        tracing::warn!(
            component = "tcp_ip_acceptor",
            peer = ?peer,
            denied_total = n + 1,
            "L4 TCP connection denied by allowlist"
        );
    }
}
```

- [ ] **Step 2: Add the real-socket integration test**

Append these two tests inside the existing `#[cfg(test)] mod tests { ... }` block in `ip_acceptor.rs`:

```rust
    use axum_server::accept::Accept;
    use tokio::net::{TcpListener, TcpStream};

    /// Accept a real loopback TCP connection and run the actual `Accept` impl.
    /// Returns whether the acceptor admitted the connection (Ok) or dropped it (Err).
    async fn accept_decision(allow: &[&str]) -> bool {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::spawn(async move { TcpStream::connect(addr).await.unwrap() });
        let (server_stream, _) = listener.accept().await.unwrap();
        let _client = client.await.unwrap();

        let acceptor = TcpIpAcceptor::new(matcher(allow));
        // Service type is irrelevant to the IP decision; use ().
        acceptor.accept(server_stream, ()).await.is_ok()
    }

    /// Allowlisted loopback peer is admitted (would proceed to TLS).
    #[tokio::test]
    async fn real_socket_allowed_peer_admitted() {
        assert!(accept_decision(&["127.0.0.0/8"]).await);
    }

    /// Non-allowlisted peer is dropped: the acceptor returns Err, which is exactly
    /// the signal axum-server uses to abort the connection before the TLS handshake.
    #[tokio::test]
    async fn real_socket_denied_peer_dropped() {
        assert!(!accept_decision(&["10.0.0.0/8"]).await);
    }
```

- [ ] **Step 3: Run the full module test suite — verify it passes**

Run: `cargo test --lib ip_acceptor`
Expected: 9 tests pass (the 7 from Task 1 plus `real_socket_allowed_peer_admitted`, `real_socket_denied_peer_dropped`).

- [ ] **Step 4: Full compile check (catches the Clone/Send/Sync bounds)**

Run: `cargo check --all-targets`
Expected: builds with no errors (no unused-item warnings now that `record_denied`/`TcpIpAcceptor` are used).

- [ ] **Step 5: Commit (source only)**

```bash
git add src/core/common/api/ip_acceptor.rs
git commit -m "feat(center): add TcpIpAcceptor pre-TLS L4 filter (admin-api-04)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Add `allow_tcp_ips` config field

**Files:**
- Modify: `src/core/center/config/mod.rs:85-115`

`CenterServerConfig` uses a **hand-written** `Default` impl (not derived), so the field must be added in two places or it won't compile.

- [ ] **Step 1: Add a serde-default round-trip test**

Append to the existing test module in `src/core/center/config/mod.rs` (find `#[cfg(test)] mod tests` in the file; if none exists in this file, add this module at the end):

```rust
#[cfg(test)]
mod allow_tcp_ips_tests {
    use super::*;

    #[test]
    fn default_allow_tcp_ips_is_empty() {
        let c = CenterServerConfig::default();
        assert!(c.allow_tcp_ips.is_empty());
    }

    #[test]
    fn yaml_without_field_defaults_to_empty() {
        // Omitting allow_tcp_ips must deserialize to an empty Vec (backward compat).
        let yaml = "grpc_addr: \"0.0.0.0:1\"\nhttp_addr: \"0.0.0.0:2\"\nprobe_addr: \"0.0.0.0:3\"\nmetrics_addr: \"0.0.0.0:4\"\n";
        let c: CenterServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(c.allow_tcp_ips.is_empty());
    }

    #[test]
    fn yaml_with_field_parses() {
        let yaml = "allow_tcp_ips:\n  - \"10.0.0.0/8\"\n";
        let c: CenterServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(c.allow_tcp_ips, vec!["10.0.0.0/8".to_string()]);
    }
}
```

- [ ] **Step 2: Run the test — verify it FAILS**

Run: `cargo test --lib allow_tcp_ips_tests`
Expected: FAIL to compile — `no field allow_tcp_ips on type CenterServerConfig`.

- [ ] **Step 3: Add the struct field**

In `src/core/center/config/mod.rs`, add to `CenterServerConfig` immediately after the `allow_admin_ips` field (line 101):

```rust
    /// L4 TCP-layer allowlist: peers not matching are dropped before the TLS
    /// handshake on the HTTPS admin listener. Independent from `allow_admin_ips`
    /// (L7). Empty/unset = allow all. Matched against the TCP peer address only.
    #[serde(default)]
    pub allow_tcp_ips: Vec<String>,
```

- [ ] **Step 4: Add the `Default` initializer**

In the same file, add to the `Default for CenterServerConfig` impl immediately after `allow_admin_ips: Vec::new(),` (line 112):

```rust
            allow_tcp_ips: Vec::new(),
```

- [ ] **Step 5: Run the test — verify it PASSES**

Run: `cargo test --lib allow_tcp_ips_tests`
Expected: 3 tests pass.

- [ ] **Step 6: Commit (source only)**

```bash
git add src/core/center/config/mod.rs
git commit -m "feat(center): add allow_tcp_ips config field (admin-api-04)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Wire the acceptor into the Center HTTPS serve path

**Files:**
- Modify: `src/core/center/cli/mod.rs` (validation ~:136, clone ~:222, HTTPS branch :295-322)

This is glue inside a large `tokio::spawn`; it is verified by `cargo check`/`clippy` and the full test suite rather than a new unit test (the serve loop is not unit-testable in isolation). The behavior it enables is already covered by Task 2's acceptor tests.

- [ ] **Step 1: Add startup validation + set-but-no-TLS WARN**

In `src/core/center/cli/mod.rs`, immediately after the existing `validate_admin_ips` line (line 136):

```rust
        crate::core::common::api::ip_acceptor::validate_tcp_ips(&config.server.allow_tcp_ips)?;
        if !config.server.allow_tcp_ips.is_empty() && config.server.admin_tls.is_none() {
            tracing::warn!(
                component = "center",
                "allow_tcp_ips is set but admin_tls is disabled; the L4 filter has no effect"
            );
        }
```

- [ ] **Step 2: Clone `allow_tcp_ips` for the spawn**

In the same file, immediately after the existing `let allow_admin_ips = config.server.allow_admin_ips.clone();` (line 222):

```rust
        let allow_tcp_ips = config.server.allow_tcp_ips.clone();
```

- [ ] **Step 3: Split the HTTPS branch on the L4 matcher**

In the same file, replace the `Some(tls) => { ... }` arm of the `match admin_tls` block (lines 296-312). The current arm is:

```rust
                Some(tls) => {
                    let cert_path = tls.cert_path();
                    let key_path = tls.key_path();
                    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
                        .await
                        .map_err(|e| {
                            tracing::error!(component = "center", error = %e, "Failed to load admin TLS cert/key");
                            anyhow::anyhow!("admin TLS load failed: {}", e)
                        })?;
                    // Logged before serve() binds the socket; a bind failure (e.g. address
                    // in use) is reported by the error returned from serve() below.
                    tracing::info!(component = "center", addr = %http_addr, "Center admin API HTTPS enabled");
                    axum_server::bind_rustls(http_addr, rustls_config)
                        .serve(make_service)
                        .await
                        .map_err(|e| anyhow::anyhow!("admin HTTPS server error: {}", e))?;
                }
```

Replace it with:

```rust
                Some(tls) => {
                    let cert_path = tls.cert_path();
                    let key_path = tls.key_path();
                    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
                        .await
                        .map_err(|e| {
                            tracing::error!(component = "center", error = %e, "Failed to load admin TLS cert/key");
                            anyhow::anyhow!("admin TLS load failed: {}", e)
                        })?;
                    // Logged before serve() binds the socket; a bind failure (e.g. address
                    // in use) is reported by the error returned from serve() below.
                    tracing::info!(component = "center", addr = %http_addr, "Center admin API HTTPS enabled");
                    // admin-api-04: L4 pre-filter — when allow_tcp_ips is set, reject
                    // unauthorized peers before the rustls handshake. When empty, serve via
                    // the original bind_rustls path (zero overhead, behavior unchanged).
                    match crate::core::common::api::ip_acceptor::build_tcp_ip_matcher(&allow_tcp_ips)? {
                        Some(m) => {
                            tracing::info!(
                                component = "center",
                                entries = allow_tcp_ips.len(),
                                "L4 TCP IP pre-filter active (HTTPS)"
                            );
                            let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(rustls_config)
                                .acceptor(crate::core::common::api::ip_acceptor::TcpIpAcceptor::new(m));
                            axum_server::bind(http_addr)
                                .acceptor(acceptor)
                                .serve(make_service)
                                .await
                                .map_err(|e| anyhow::anyhow!("admin HTTPS server error: {}", e))?;
                        }
                        None => {
                            axum_server::bind_rustls(http_addr, rustls_config)
                                .serve(make_service)
                                .await
                                .map_err(|e| anyhow::anyhow!("admin HTTPS server error: {}", e))?;
                        }
                    }
                }
```

- [ ] **Step 4: Compile + lint**

Run: `cargo check --all-targets`
Expected: no errors.

Run: `cargo clippy --all-targets`
Expected: no new warnings on `center/cli/mod.rs` or `ip_acceptor.rs`.

- [ ] **Step 5: Run the full affected test suite**

Run: `cargo test --lib ip_acceptor && cargo test --lib allow_tcp_ips_tests`
Expected: all pass (9 + 3).

- [ ] **Step 6: Format**

Run: `cargo fmt --all`
Expected: no diff left uncommitted that contradicts house style.

- [ ] **Step 7: Commit (source only)**

```bash
git add src/core/center/cli/mod.rs
git commit -m "feat(center): wire L4 TcpIpAcceptor into HTTPS admin serve path (admin-api-04)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Operator documentation

**Files:**
- Modify: `skills/02-features/02-config/00-controller-config.md` (config table, after the `allow_admin_ips` row ~:105)

The L7 `allow_admin_ips` is documented in this admin-config reference; document the new L4 field right after it so operators see both layers together.

- [ ] **Step 1: Add the `allow_tcp_ips` table row**

In `skills/02-features/02-config/00-controller-config.md`, immediately after the `allow_admin_ips` row (line 105), add:

```markdown
| `allow_tcp_ips` | `Vec<String>` | `[]` (allow all) | **Center HTTPS only.** L4 pre-filter CIDR allowlist: peers not matching are dropped **before the rustls handshake** (saves TLS-handshake CPU, mitigates TLS-handshake DoS). **Independent** from `allow_admin_ips` (L7) — separate list. **Unset or empty = allow all** (backward compatible). Matched against the **TCP peer address only**. **Precondition:** correct only when the kernel TCP peer is the real client — behind an SNAT load balancer / terminating proxy `peer_addr()` is the LB IP, so the filter either no-ops (LB allowlisted) or drops all traffic (LB not allowlisted). Recommended operator invariant: keep `allow_tcp_ips ⊇ allow_admin_ips` (a stricter L4 silently drops L7-allowed IPs with no 403). Ignored when `admin_tls` is unset (plaintext) — a startup WARN is logged. Observability: the `edgion_l4_tcp_denied_total` counter on the metrics listener. |
```

- [ ] **Step 2: Verify the doc-consistency check still passes**

Run: `make check-agent-docs`
Expected: pass.

- [ ] **Step 3: Commit (docs only)**

```bash
git add skills/02-features/02-config/00-controller-config.md
git commit -m "docs(center): document allow_tcp_ips L4 allowlist (admin-api-04)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Final verification + task-file cleanup

**Files:**
- Delete: `tasks/todo/admin-api-04-center-l4-ip-acceptor.md` (after completion, per project task lifecycle)

- [ ] **Step 1: Full pre-commit gate**

Run, in order (steps 2-4 may run in parallel):
```bash
cargo fmt --all
cargo check --all-targets
cargo clippy --all-targets
make check-agent-docs
```
Expected: all pass. Fix and re-run any that fail.

- [ ] **Step 2: Manual smoke (optional, if a Center HTTPS test env exists)**

With `admin_tls` configured and `allow_tcp_ips: ["127.0.0.0/8"]`, start Center and confirm: a `127.0.0.1` client completes TLS to the admin API; a client from a non-allowlisted IP has its connection dropped before TLS (e.g. `openssl s_client -connect <center-https>` from a disallowed host closes immediately). Check `edgion_l4_tcp_denied_total` increments on Center's `/metrics`.

- [ ] **Step 3: Remove the completed task file**

```bash
rm tasks/todo/admin-api-04-center-l4-ip-acceptor.md
```
(Per the project's task lifecycle, `ls tasks/todo/` is the source of truth; completed tasks are deleted.)

- [ ] **Step 4: Final commit (source + task removal)**

```bash
git add -A src/ tasks/
git commit -m "chore(center): complete admin-api-04 L4 TCP IP pre-filter

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review Notes (author check, not for the executor)

- **Spec coverage:** config field (T3), validation + WARN (T4), self-contained module + matcher + predicate (T1), acceptor + `Accept` impl + denial metric/log (T2), HTTPS-branch wiring with empty=passthrough (T4), unit + real-socket tests (T1/T2), metric-name const + stability test (T1), operator docs incl. LB precondition + L4⊇L7 invariant + metric (T5). PROXY-protocol unsupported and the L4⊇L7 invariant are documented (T5 + module doc-comment + spec), not code-enforced — by design.
- **Independence:** `ip_acceptor.rs` imports only `core::common::matcher`, never `ip_allowlist`. Separate `record_denied` counter. ✓
- **Type consistency:** `build_tcp_ip_matcher` returns `Option<Arc<IpRadixMatcher>>`; `TcpIpAcceptor::new(Arc<IpRadixMatcher>)`; `Accept<TcpStream, S>` with `Stream=TcpStream`, `Service=S`, `Future=Ready<io::Result<(TcpStream,S)>>` — consistent across T1/T2/T4.
- **No placeholders:** all code shown in full; commands have expected output.
