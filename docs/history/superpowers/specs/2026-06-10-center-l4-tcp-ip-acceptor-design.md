# Center L4 TCP IP Pre-Filter (reject before TLS handshake) — Design

- Date: 2026-06-10
- Task: `tasks/todo/admin-api-04-center-l4-ip-acceptor.md`
- Depends on (already landed): admin-api-01 (Center HTTPS, `admin_tls`), admin-api-02 (L7 `allow_admin_ips`).
- Security finding: `tasks/security-audit/04-admin-api.md` — H2.
- Reviewed: 2026-06-10, three parallel review agents (axum-server API correctness, codebase integration, security/behavior). Their corrections are folded in below.

## Background

`admin-api-02` added an IP allowlist to the Center / Controller Admin API, but it is
**L7 enforcement**: the decision input is the kernel TCP peer IP (unspoofable), yet the
enforcement point is an axum HTTP middleware. An unauthorized peer still completes the
**TCP handshake + TLS handshake** and sends a fully parsed HTTP request before getting a 403.

Center is the federation hub, is HTTPS, and is reachable across clusters / the public
network. The TLS handshake (asymmetric crypto) is CPU-expensive and a classic DoS
amplification point. This design adds an **L4 pre-filter** to Center's HTTPS branch: peers
not in a dedicated `allow_tcp_ips` allowlist have their TCP connection dropped **before** the
rustls handshake.

### What this does and does not save (honest scope)

The filter runs inside the per-connection task that `axum_server` spawns. The actual
per-connection order is: kernel TCP 3-way handshake completes → `accept()` syscall
(`server.rs:288`) → `make_service.make_service(socket_addr)` clones the per-conn router
(`server.rs:298`) → `tokio::spawn` (`server.rs:308`) → **our acceptor runs** → on `Err`, the
connection is dropped before any TLS work.

So a rejected connection **still** costs: the TCP handshake, an `accept()`, a `make_service`
router clone, a task spawn, and one `peer_addr()` syscall. What it eliminates is the **TLS
handshake CPU** only. This is **TLS-handshake-DoS mitigation, not connection-level DoS
mitigation**. The primary hard-isolation defense remains K8s NetworkPolicy (a pre-accept
kernel/eBPF drop); this feature is application-layer defense in depth behind it.

## Goal

On Center's HTTPS admin listener, after `accept()` produces the TCP connection and
**before** the rustls handshake, decide on the peer IP using a dedicated L4 allowlist
(`allow_tcp_ips`). Peers not in the allowlist have their TCP connection dropped silently
(connection closed with no TLS/application bytes sent back — FIN or RST depending on
whether the client's ClientHello already sits unread in the socket buffer; with a TLS
client it is usually RST, since the ClientHello arrives before our acceptor runs).

## Scope

- **Center HTTPS branch only** (`axum_server` rustls path).
- Plaintext HTTP branch: **not done** — no expensive handshake, so the cheap HTTP parse
  before the L7 middleware is already roughly L4-equivalent.
- Controller: **not done** — ClusterIP, intra-cluster only, no TLS.

## Preconditions / network topology (READ FIRST — operator-critical)

The decision input is `stream.peer_addr()` = the **kernel TCP peer**. The feature is correct
**only when the kernel TCP peer is the real client**. If anything rewrites the L4 source
address in front of Center, the filter misbehaves:

- **SNAT load balancer** (e.g. K8s `Service type=LoadBalancer` with
  `externalTrafficPolicy: Cluster`, most cloud L7 ingress / ALB, any terminating reverse
  proxy): `peer_addr()` is the LB / node IP, not the client.
  - LB IP **is** in `allow_tcp_ips` → the filter is a **no-op** (every client looks like the
    LB and passes) → false sense of security.
  - LB IP **is not** in `allow_tcp_ips` → **total outage** (all connections dropped
    pre-TLS), with no 403 and only a rate-limited WARN to diagnose from.

**Correct deployments:** Center reachable directly at L4 — ClusterIP within the federation,
a client-IP-preserving path (`externalTrafficPolicy: Local` / direct NodePort), or a
PROXY-protocol-terminated front end (not yet supported — see below).

This caveat already applies to the L7 feature (which deliberately ignores `X-Forwarded-For`),
but L4 makes the failure mode sharper: the drop happens before any HTTP, so no XFF fallback
is even possible. **`edgion_l4_tcp_denied_total` (below) is the primary signal to detect a
misconfigured front end — a 100%-denial spike right after a deploy.**

**PROXY protocol is not supported.** The PROXY header arrives after TCP accept and before
TLS — exactly where this acceptor sits — but parsing it is out of scope here. The acceptor is
the natural future extension point for PROXY-protocol support; until then the preconditions
above hold.

## Independence principle

The L4 and L7 allowlists are **independent features**. They do not share a config field, an
enforcement predicate, a logging counter, or a module. The only shared code is the
lower-level library `core::common::matcher` (the IP radix tree + CIDR helpers), which both
features build on independently. The L4 module does **not** depend on the L7 `ip_allowlist`
module. This costs ~15 lines of duplicated matcher-build logic plus a few lines each of
validation and the rate-limited logger; that is the accepted price of feature independence.

### Recommended operational invariant (documentation only)

Operators should keep **`allow_tcp_ips ⊇ allow_admin_ips`** (L4 a superset of L7). Mismatch
consequences:

- L4 **stricter** than L7 (L4 ⊉ L7): an IP the operator allowed at L7 is silently dropped
  pre-TLS by L4 — no 403, hard to debug.
- L4 **looser** than L7: wasted TLS handshakes followed by an L7 403 (benign, but defeats
  the CPU goal).

By explicit design decision this invariant is **documented, not enforced in code** — the two
lists stay fully independent. (If a future need arises, a startup advisory WARN on
non-superset could be added without breaking independence.)

## Configuration (new dedicated field)

Add to `CenterServerConfig` (`src/core/center/config/mod.rs`, struct at line 85):

```rust
/// L4 TCP-layer allowlist: peers not matching are dropped before the TLS
/// handshake. Independent from `allow_admin_ips` (L7). Empty = allow all.
#[serde(default)]
pub allow_tcp_ips: Vec<String>,
```

- `CenterServerConfig` uses a **hand-written `Default` impl** (lines 104-115), not
  `#[derive(Default)]`. Add the matching initializer there or it will not compile:

  ```rust
  allow_tcp_ips: Vec::new(),
  ```

- Default empty `Vec`, backward compatible. **Empty = allow-all is a deliberate fail-open
  default** (a new field must not break existing deploys; matches L7). This must be called
  out prominently in operator docs as a security-relevant default.
- Fully independent from the L7 `allow_admin_ips`: two lists, separately configurable.
- Startup fail-fast validation: add `validate_tcp_ips(&config.server.allow_tcp_ips)?`
  immediately after the existing `validate_admin_ips` call in `center/cli/mod.rs:136`.
- **Startup WARN when set-but-ineffective:** if `allow_tcp_ips` is non-empty while
  `admin_tls` is `None` (plaintext), the field is silently ignored (L4 only mounts on the
  HTTPS branch). Emit a WARN at startup so a silently-dropped security config is visible:
  `"allow_tcp_ips is set but admin_tls is disabled; the L4 filter has no effect"`. Place it
  in the synchronous main body immediately after the `validate_tcp_ips` call (after
  `cli/mod.rs:136`), where both `config.server.allow_tcp_ips` and `config.server.admin_tls`
  are in scope and it fires exactly once (not inside the spawn):

  ```rust
  if !config.server.allow_tcp_ips.is_empty() && config.server.admin_tls.is_none() {
      tracing::warn!(component = "center",
          "allow_tcp_ips is set but admin_tls is disabled; the L4 filter has no effect");
  }
  ```

## New module: `src/core/common/api/ip_acceptor.rs` (self-contained)

Does **not** import the L7 `ip_allowlist` module. Both build only on
`core::common::matcher` (`ip_radix_tree::IpRadixMatcher`, `ip_cidr_helpers`).

Imports (none are shown elsewhere — list them explicitly so the module compiles):

```rust
use std::io;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use metrics::counter;
use tokio::net::TcpStream;

use crate::core::common::matcher::ip_cidr_helpers::{to_cidr_format, validate_ip_or_cidr};
use crate::core::common::matcher::ip_radix_tree::IpRadixMatcher;

/// Metric name, kept as a const to match the codebase's name-as-const discipline
/// (every `counter!`/`gauge!` call site uses a const, never a string literal).
/// Note: prefix is `edgion_l4_` (not `edgion_fed_`) — this is an admin-listener L4
/// metric, not a federation metric.
const DENIED_TOTAL: &str = "edgion_l4_tcp_denied_total";
```

Contents:

- `pub fn validate_tcp_ips(cidrs: &[String]) -> anyhow::Result<()>` — uses
  `ip_cidr_helpers::validate_ip_or_cidr`; no matcher built.
- `pub fn build_tcp_ip_matcher(cidrs: &[String]) -> anyhow::Result<Option<Arc<IpRadixMatcher>>>`
  — `Ok(None)` when the list is empty (caller must not mount the acceptor); `Err` on a
  malformed CIDR (defense in depth; load already validates). Built **once at startup** — no
  hot reload (same limitation as L7).
- `fn tcp_ip_allowed(matcher: &IpRadixMatcher, ip: IpAddr) -> bool` — the **pure decision
  predicate**: `matcher.matched_group(&ip.to_canonical()).is_some()`. This is the unit-test
  target. Mirrors L7 `ip_allowlist.rs:106-108`.
- `struct TcpIpAcceptor { matcher: Arc<IpRadixMatcher> }`. **Must derive `Clone`** (and the
  matcher must be `Send + Sync` — `Arc<IpRadixMatcher>` is, since `IpRadixMatcher: Send + Sync`):
  `axum_server`'s `serve` requires `Acc: Clone + Send + Sync + 'static`, and
  `RustlsAcceptor`'s `Clone` is conditional on its inner acceptor being `Clone`.

  ```rust
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

      fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
          match stream.peer_addr() {
              Ok(addr) if tcp_ip_allowed(&self.matcher, addr.ip()) => {
                  std::future::ready(Ok((stream, service)))
              }
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
  ```

  A synchronous `std::future::Ready` future, mirroring the crate's own `NoDelayAcceptor`
  (`accept.rs:57-65`). **Keep the inner accept synchronous/non-blocking:**
  `RustlsAcceptor`'s `handshake_timeout` wraps **only** the TLS future, **not** the inner
  acceptor (`tls_rustls/future.rs:73-97`), so async work added here would be unprotected by
  any timeout. Add a code comment to that effect.

### Why approach A works (verified against axum-server 0.8.0)

`Accept<I, S>` (`src/accept.rs:10-22`) takes the raw `TcpStream` as input.
`RustlsAcceptor<A>` (`src/tls_rustls/mod.rs:148, 157-172`) wraps an inner acceptor and calls
`self.inner.accept(stream, service)` **first**; the ordering is enforced in
`RustlsAcceptorFuture` (`src/tls_rustls/future.rs:71-97`), which polls the inner future and
only constructs `TlsAcceptor` / runs the handshake on inner `Ok`. On inner `Err` it returns
`Err` immediately, never touching TLS. The server loop drops the connection at
`server.rs:309` (`if let Ok((stream, send_service)) = acceptor.accept(...).await`); the `Err`
is silently swallowed by axum-server, so our in-acceptor `record_denied` is the **only**
observability for denials. `peer_addr()` on `TcpStream` is synchronous. `service` passes
through untouched, so the L7 fallback's `ConnectInfo<SocketAddr>` (derived earlier from
`make_service(socket_addr)`, independent of the acceptor) still flows.

Approach B (hand-rolled `tokio::net::TcpListener` + `tokio_rustls` + hyper) is therefore
unnecessary and retained only as a documented fallback.

## Observability: denied counter + rate-limited log

Both live in `ip_acceptor.rs`, independent from L7's counter/label:

- **Metric:** a label-less Prometheus counter `edgion_l4_tcp_denied_total` (referenced via
  the `DENIED_TOTAL` const above), emitted via the `metrics` crate facade
  (`counter!(DENIED_TOTAL).increment(1)`), incremented on **every** denied connection. No
  peer-IP label (cardinality discipline — see `fed_metrics.rs`). Verified end-to-end: Center
  installs a process-wide Prometheus recorder at startup (`metrics_api::install_global_recorder`,
  `center/cli/mod.rs`) and `create_metrics_router()` (`center/api/mod.rs:178`) renders that
  same handle on the metrics listener, so any in-process `counter!` is scraped — exactly how
  `fed_metrics` counters surface today. No `describe_counter!` step is required (the codebase
  uses none). This is the primary signal for the LB-misconfiguration outage (100%-denial
  spike) and for scanning attacks. Add a one-line name-stability test asserting
  `DENIED_TOTAL == "edgion_l4_tcp_denied_total"`, mirroring the `fed_metrics` name tests.
- **Log:** rate-limited WARN — a dedicated static `AtomicU64` (1st, then every 256th),
  component label `tcp_ip_acceptor` (no "admin"), including the peer IP via a structured
  `?peer` field (an `IpAddr`, not string-interpolated → no log injection). The counter
  increments unconditionally; only the log line is sampled.

```rust
fn record_denied(peer: Option<IpAddr>) {
    counter!(DENIED_TOTAL).increment(1);
    static DENIED: AtomicU64 = AtomicU64::new(0);
    let n = DENIED.fetch_add(1, Ordering::Relaxed);
    if n == 0 || n % 256 == 0 {
        tracing::warn!(component = "tcp_ip_acceptor", peer = ?peer, denied_total = n + 1,
            "L4 TCP connection denied by allowlist");
    }
}
```

## Wiring (Center HTTPS branch, `center/cli/mod.rs`)

Two parts:

1. **Pre-spawn clone** (mirror `cli/mod.rs:222` which clones `allow_admin_ips`), so the field
   can move into the `async move` spawn at line 233:
   ```rust
   let allow_tcp_ips = config.server.allow_tcp_ips.clone();
   ```

2. **HTTPS branch** (`cli/mod.rs:295-322`). The replacement must **preserve the existing
   error handling and logging** — `tls.cert_path()` / `tls.key_path()`, the `.map_err(...)?`
   wrappers, the fully-qualified `axum_server::tls_rustls::RustlsConfig`, and the
   "HTTPS enabled" `tracing::info!`. Only the `serve` call splits on the matcher:

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
       tracing::info!(component = "center", addr = %http_addr, "Center admin API HTTPS enabled");
       match crate::core::common::api::ip_acceptor::build_tcp_ip_matcher(&allow_tcp_ips)? {
           Some(m) => {
               tracing::info!(component = "center", entries = allow_tcp_ips.len(),
                   "L4 TCP IP pre-filter active (HTTPS)");
               let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(rustls_config)
                   .acceptor(crate::core::common::api::ip_acceptor::TcpIpAcceptor::new(m));
               axum_server::bind(http_addr)
                   .acceptor(acceptor)
                   .serve(make_service)
                   .await
                   .map_err(|e| anyhow::anyhow!("admin HTTPS server error: {}", e))?;
           }
           None => {
               // L4 inactive: original path, unchanged.
               axum_server::bind_rustls(http_addr, rustls_config)
                   .serve(make_service)
                   .await
                   .map_err(|e| anyhow::anyhow!("admin HTTPS server error: {}", e))?;
           }
       }
   }
   ```

`make_service` (built once at `cli/mod.rs:294`) is shared by both sub-branches and both the
HTTP/HTTPS branches; `ConnectInfo<SocketAddr>` continues to reach the L7 middleware. The
spawn closure already returns `Result<(), anyhow::Error>`, so `build_tcp_ip_matcher(...)?` is
consistent with the existing `build_admin_ip_matcher(...)?` at line 275.

`handshake_timeout` parity: `RustlsAcceptor::new(config)` uses the same default
`handshake_timeout` (10s) that `bind_rustls` applies internally — no regression on the
custom path.

## Edge cases

- `peer_addr()` fails → **fail-closed** (drop), consistent with the L7 "missing ConnectInfo
  ⇒ deny" rule. `getpeername` fails only when the socket is already torn down, so no
  legitimate allow-case is lost.
- IPv4-mapped IPv6 peers are normalized via `to_canonical()` before matching (a `::`-bound
  dual-stack listener reports IPv4 clients as `::ffff:a.b.c.d`).
- Graceful shutdown is unaffected (the acceptor does not touch the `Handle`/watcher path).

## Testing (unit + real-socket integration)

- **Unit tests** on `tcp_ip_allowed` / `build_tcp_ip_matcher` / `validate_tcp_ips`: IPv4
  in/out of CIDR, IPv6 in CIDR, IPv4-mapped IPv6 matches an IPv4 CIDR, empty list ⇒ `None`,
  malformed CIDR ⇒ `Err`.
- **Integration test**: vary the **allowlist**, not the client source IP (binding a second
  loopback alias like `127.0.0.2` is not portable on CI / macOS). Single `127.0.0.1` client
  against a real HTTPS listener wrapped with `TcpIpAcceptor`:
  - **Denied case**: `allow_tcp_ips = ["10.0.0.0/8"]` (the `127.0.0.1` client never matches).
    Drive a real `tokio_rustls::TlsConnector::connect(...)` and assert it returns `Err` of a
    connection-closed kind — accept **any** of `ConnectionReset | UnexpectedEof | BrokenPipe`
    (do not pin a single `ErrorKind`; FIN vs RST is nondeterministic per item above).
  - **Allowed case**: same server/client with `allow_tcp_ips = ["127.0.0.0/8"]`; assert the
    TLS handshake succeeds and a minimal HTTP round-trip returns. The paired positive
    assertion is what proves the denial came from the L4 filter (only the allowlist differs).

## Files changed

- `src/core/center/config/mod.rs` — add `allow_tcp_ips` field **and** the matching line in
  the hand-written `Default` impl (lines 104-115).
- `src/core/center/cli/mod.rs` — startup validation (after :136), set-but-no-TLS WARN,
  pre-spawn clone (near :222), HTTPS-branch wiring (:295-322).
- **New** `src/core/common/api/ip_acceptor.rs` — module + unit tests + `record_denied`
  (metric + rate-limited log).
- `src/core/common/api/mod.rs` — `pub mod ip_acceptor;` (mirrors `pub mod ip_allowlist;` at :2).
- Integration test file (location decided during implementation).
- Operator docs — document the empty=allow-all fail-open default, the
  `allow_tcp_ips ⊇ allow_admin_ips` invariant, the LB/proxy precondition, and the
  `edgion_l4_tcp_denied_total` metric.
- Untouched: `ip_allowlist.rs`, the L7 middleware, the Controller.

## Out of scope

Plaintext HTTP branch, Controller, CLI flag (Center is YAML-configured; add a flag later if
needed), L4 deny-list (allow-only), PROXY-protocol parsing (documented future extension
point), code-enforced L4⊇L7 invariant (documented only), hot reload of the matcher.
```