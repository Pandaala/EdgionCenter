# Center Admin API HTTPS Support — Design Spec

- **Date**: 2026-06-09
- **Source task**: `tasks/todo/admin-api-01-center-https.md`
- **Security finding**: H2 (the original `tasks/security-audit/` files are lost — not on disk
  or in git history — so the finding below was **independently re-derived from the current
  code**, with file:line evidence, rather than quoted from the audit)
- **Related**: `tasks/todo/admin-api-02-allow-admin-ips.md` (parallel), `tasks/todo/admin-api-03-controller-https.md` (later, reuses this work)

## Security Finding H2 (re-derived from code)

**H2 — Plaintext-HTTP admin transport.** Both the Center and Controller Admin HTTP APIs are
served over unencrypted HTTP with no TLS option in code:

- Center plaintext serve: `src/core/center/cli/mod.rs:252-253` (`TcpListener::bind(http_addr)`
  + `axum::serve`), default bind `0.0.0.0:12201` (`center/config/mod.rs:100`) — all interfaces.
- Controller plaintext serve: `src/core/controller/api/mod.rs:585-586` (same pattern).
- No `admin_tls` / `AdminTlsConfig` / `axum_server` / `bind_rustls` exists anywhere under
  `src/core/` today — the TLS deps named in the task are not yet wired.

What actually crosses the wire in clear (the reason this is High, not cosmetic):

- Login **username/password** in the request JSON body (`local_auth/handlers.rs:33-36`,
  consumed `:90`).
- The HS256 **JWT in the response body** (`LoginResponse.token`, `handlers.rs:171-177`).
- The same JWT as a bearer-equivalent **`edgion_token` cookie** (`handlers.rs:167-177`),
  re-presented on every subsequent request (`unified_auth/mod.rs:338-347`) and replayable
  until `exp`.

A passive on-path observer therefore captures admin credentials **and** a replayable admin
JWT. **Severity: High for Center** (designed for external / cross-cluster exposure, binds
`0.0.0.0`, gains a browser Dashboard); **Medium for Controller** (normally ClusterIP; threat
is in-cluster sniffing / compromised sidecar). Controller TLS is deferred to task 03; this
task addresses Center.

> Note: the `H2` label itself is inferred from the three tasks' cross-references plus the
> code; the severities above are this analysis's own justification from the evidence, not
> quotations from the lost audit.

### What counts as "H2 closed"

The optional-TLS mechanism is **operator-must-act, not default-secure**: with default config
Center still serves plaintext and the finding remains fully open. Shipping the switch delivers
the **mechanism**; H2 is considered closed for a Center deployment only when one of:

1. **`admin_tls` enabled** with a mounted server-cert Secret — Center itself terminates HTTPS.
   The serve path is an either/or branch (Section 2): when `admin_tls` is set there is **no
   plaintext admin listener at all**, so TLS **alone fully closes H2** here. The `admin-api-02`
   IP allowlist is valuable defense-in-depth but is **not** a precondition for H2 closure in
   this case — H2 is a confidentiality-in-transit finding, the allowlist is access control; do
   not gate one on the other. **Or**
2. **TLS-terminating front proxy** (Ingress / LB) documented as the supported topology, fronting
   the still-plaintext admin listener. Here the allowlist (or equivalent network policy
   restricting who can reach the plaintext listener directly, bypassing the proxy) **is**
   required for closure, because the listener itself remains cleartext.

This task additionally does **not** make TLS mandatory, add an HTTP→HTTPS redirect, or emit
HSTS — so a plaintext listener can still coexist unless the operator removes it. Wiring the
default deployment manifests / Helm values to satisfy (1) or (2) is out of scope for this code
task but is the closure condition; recorded here so the finding is not marked closed merely
because the option exists.

**Residual token-capture risk (relevant even after TLS closes the transit gap).** The threat
H2 guards against is capture of a replayable admin token, so two adjacent properties are worth
recording: (a) **no revocation path** — tokens are valid until `exp` with no `jti` / revocation
list / IP binding (`unified_auth/mod.rs:498-523`), so a leaked token can only be invalidated by
rotating the shared `jwt_secret`, which logs out every session; (b) **HS256 shared secret** —
tokens are signed symmetrically with `jwt_secret` (`handlers.rs:156`), so a weak secret is
brute-forceable offline to mint arbitrary admin tokens. Both are out of scope for this task but
bound how much residual risk TLS removes.

## Goal

Let the Center Admin API serve over HTTPS. When TLS is not configured, fall back to the
current plain-HTTP path with full backward compatibility. TLS is **optional and operator-
configurable**: the operator can either terminate TLS in Center itself, or keep Center on
HTTP behind a front proxy (Ingress / LB) that terminates TLS.

## Scope Decisions

Three decisions were settled during brainstorming and fix the boundaries of this task:

1. **Server-side TLS only — no mTLS.** Center's primary future consumer is a browser-based
   web console, which cannot easily present client certificates. The `ca` / client-cert
   verification field from the original task is **dropped**. `AdminTlsConfig` exposes only
   `cert` + `key`. If a non-browser cross-cluster client ever needs mTLS, it is a separate
   future task.

2. **`cookie_secure` stays independent; add a warning only.** The login/logout `Secure`
   cookie flag remains controlled by `LocalAuthConfig.cookie_secure` (default `true`). We do
   **not** auto-derive `Secure` from whether Center itself has TLS, because the front-proxy
   case (Center on HTTP, browser on HTTPS) legitimately needs `Secure` cookies while Center
   sees plain HTTP. Runtime behavior is unchanged; we only add a startup WARN on the one
   genuinely contradictory combination (see Section 4).

3. **Only the admin listener gets TLS.** Center runs three independent listeners: admin
   (`http_addr`), probe (`probe_addr`), and metrics (`metrics_addr`). `admin_tls` scopes to
   the **admin listener only**. Probe (K8s liveness/readiness) and metrics (Prometheus
   scrape) are cluster-internal, unauthenticated, and stay plain HTTP. The original task's
   note about switching the probe `scheme` to HTTPS does **not** apply here — the probe is a
   separate socket and is unaffected by admin TLS.

## Configuration Schema (YAML)

Center config is YAML (not TOML, contrary to the original task wording). The new optional
section lives under `server`:

```yaml
server:
  http_addr: "0.0.0.0:12201"
  probe_addr: "0.0.0.0:12200"      # unaffected — always HTTP
  metrics_addr: "0.0.0.0:12290"    # unaffected — always HTTP

  admin_tls:                        # optional; omit -> current HTTP path
    cert: "certs/admin/server.crt"  # PEM server certificate, required
    key:  "certs/admin/server.key"  # PEM private key, required
```

Omitting `admin_tls` reproduces today's behavior exactly.

## Architecture & Components

### 1. `AdminTlsConfig` struct

- **Location**: `src/core/common/config/mod.rs` (in `common`, so Controller can reuse it
  later per task 03).
- **Not** a reuse of `ConfSyncTlsConfig`: that struct requires `cert` + `key` + `ca` all
  mandatory, which does not fit a server-side TLS listener with no client-cert verification.
- **Fields**: `cert: String`, `key: String`.
- **Path resolution**: `cert_path()` / `key_path()` resolve relative paths against
  `work_dir()` via `crate::types::work_dir().resolve(...)`, identical to `ConfSyncTlsConfig`.
- **Custom `Debug`**: print `key` as `"***"` to keep the private-key path out of logs
  (same pattern as `ConfSyncTlsConfig`'s `Debug`).
- **`validate()`**: bail if `cert` or `key` is empty (after trim). There is no central
  `CenterConfig::validate()` aggregate hook — Center validates piecemeal in the serve path, so
  call this synchronously next to the existing `config.grpc_security.validate()?`
  (`cli/mod.rs:123-126`), which returns via `?` from `serve()` before any task is spawned. The
  natural split: cheap sync `validate()` in the main path pre-spawn; async `from_pem_file` cert
  load inside the `Result`-returning closure (Section 3).
- **Wiring**: add `admin_tls: Option<AdminTlsConfig>` to `CenterServerConfig`. That struct
  already carries struct-level `#[serde(default)]` plus a manual `Default` impl
  (`center/config/mod.rs:83-105`), so the new field needs no field-level attribute; an omitted
  `admin_tls` deserializes to `None`. The `Default` impl **must** be updated to initialize
  `admin_tls: None` — the compiler enforces this, so there is no silent serde trap.

### 2. Serve-path branch

In `src/core/center/cli/mod.rs`, the `http_handle` task (currently around line 212) is an
`async move` closure that captures **pre-cloned locals** (`auth_config`, `local_auth_config`,
`http_addr`, ...) — it does **not** capture `config`. So clone the TLS config out **before** the
spawn, mirroring `let auth_config = config.auth.clone();` (`cli/mod.rs:208`):
`let admin_tls = config.server.admin_tls.clone();`. Then, after `app` is composed, branch on
that captured `admin_tls`:

- **`Some(tls)`** — HTTPS:
  - The process-level rustls `CryptoProvider` (ring) is **already installed**: `edgion_center`
    calls `crate::core::common::startup::init_crypto()` at `src/bin/edgion_center.rs:6`. No
    additional provider install is needed in the serve path.
  - `axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await`
  - `axum_server::bind_rustls(http_addr, rustls_config).serve(make_service).await`
  - TLS uses rustls **safe defaults** (TLS 1.2 + 1.3, AEAD suites; no SSLv3/TLS1.0/1.1) from
    the installed ring provider's `ServerConfig`. Explicit min-version / cipher pinning is
    intentionally not configured in this task.
  - Startup log: `INFO component="center" ... "Center admin API HTTPS enabled"`.
- **`None`** — HTTP (current path): `TcpListener::bind(http_addr)` + `axum::serve(listener, make_service)`.

This is the **first actual use of `axum_server` in `src/`** (the dependency already exists in
`Cargo.toml`).

**Coordination with `admin-api-02` (parallel task — client IP allowlist).** Task 02 requires
the client `SocketAddr`, which means the make-service must be
`app.into_make_service_with_connect_info::<SocketAddr>()` rather than plain
`into_make_service()`. Both branches (HTTP `axum::serve` and HTTPS `bind_rustls`) must use the
**same** connect-info make-service, otherwise the IP allowlist silently no-ops (or panics on a
missing `ConnectInfo` extractor) under TLS. Whichever of tasks 01/02 lands second must verify
both branches are aligned. If task 01 lands first, prefer wiring connect-info now so task 02
only adds the middleware.

**Graceful shutdown semantics differ between branches.** `axum::serve` and `axum_server::Server`
have different shutdown wiring (the latter needs a `Handle`). This task does not add graceful
shutdown to either branch: the existing `tokio::select!` (`cli/mod.rs:283-296`) treats admin-
listener exit as fatal and tears the process down, which is the intended fail-stop behavior.
Noted so the asymmetry is a conscious choice, not an oversight.

### 3. Error handling

The current `http_handle` is a `tokio::spawn` whose async closure returns `()` and uses
`.unwrap()` on bind/serve. The `tokio::select!` arm at `cli/mod.rs:292-294` already fires on
**any** task exit (including today's panic, which surfaces as a `JoinError`) and does
`tracing::error!("HTTP server exited: {:?}", r)` before returning a constant `Err`.

For the TLS branch, certificate load failure (missing path, malformed PEM) must **not** panic:

- Change the spawn closure's return type from `()` to `anyhow::Result<()>` so `from_pem_file` /
  bind errors return with `?` instead of `.unwrap()`. This needs **no change to the `select!`
  arm**: `r` becomes `Result<anyhow::Result<()>, JoinError>`, both arms are `Debug`, and the
  existing `{:?}` will now print the anyhow cause chain.
- Optionally also log `ERROR` with `error = %e` **inside the closure** before returning, for a
  cleaner single-cause line than the nested `{:?}` the arm prints. (Good practice, not required
  for correctness.)
- The returned `Err` exits the task, the `select!` arm fires, and the process tears down
  (fail-stop → visible crashloop in K8s, not a half-up Center).

Bring the HTTP branch to the same `Result`-returning shape so both branches behave
consistently.

### 4. `cookie_secure` ↔ TLS reconciliation (warning only)

During Center auth assembly at startup, run a small **pure decision function** that takes
`(admin_tls_present: bool, cookie_secure: bool)` and returns which (if any) warning to emit.
Keeping it a pure function (rather than inline `tracing::warn!`) makes it unit-testable
without a tracing capture layer. Two warned cases:

- **Contradiction**: `admin_tls.is_some()` (Center itself terminates HTTPS) **and**
  `cookie_secure == false` → `WARN`: admin TLS is enabled but the auth cookie is non-`Secure`;
  recommend enabling `cookie_secure`.
- **Silent-drop risk**: `admin_tls.is_none()` **and** `cookie_secure == true` (both are the
  defaults) → informational `WARN`: the auth cookie is `Secure` but the admin listener is
  plain HTTP; if Center is reached **directly** over `http://` the browser will silently drop
  the session cookie (login returns 200 with a token in the body but the cookie never sticks).
  Ensure a TLS-terminating proxy fronts it. This is warn-only on purpose: hard-failing would
  break the legitimate front-proxy topology, since Center cannot know whether a proxy exists.
  Note: edgion-cli's bearer-token flow is unaffected (the token is also returned in the body);
  only browser cookie sessions are at risk.

`src/core/common/local_auth/handlers.rs` is **not** modified; runtime cookie behavior is
unchanged — these are startup diagnostics only.

## Data Flow

1. Center loads YAML config → `CenterServerConfig.admin_tls: Option<AdminTlsConfig>`.
2. Startup validates `admin_tls` (if present) and runs the `cookie_secure` warning check.
3. The `http_handle` task branches to `bind_rustls` (TLS) or `axum::serve` (HTTP).
4. Probe and metrics tasks are untouched and continue on plain HTTP.

## Testing

- **`AdminTlsConfig::validate()`**: empty `cert` → error; empty `key` → error; both
  non-empty → ok.
- **`Debug` redaction**: formatted output contains `***` and never the key path in clear.
- **Config deserialization**: YAML with and without `admin_tls` both parse correctly
  (`Option` / `serde(default)` semantics).
- **TLS load smoke test (happy path)**: generate a temporary self-signed PEM cert+key with
  `rcgen` (dependency already present) and assert `RustlsConfig::from_pem_file` loads it
  successfully. End-to-end HTTPS listener startup belongs to integration testing.
- **TLS load negative tests**: assert graceful `Err` (not panic) for (a) a malformed/garbage
  PEM file and (b) a nonexistent cert/key path. These directly validate the Section 3
  "must not panic" requirement.
- **Cookie/TLS warning predicate**: unit-test the pure decision function from Section 4 for
  all four `(admin_tls_present, cookie_secure)` combinations — contradiction case warns,
  silent-drop case warns, the two safe combinations stay silent.

## Client-side impact (edgion-cli) — out of scope, documented

When Center is switched to HTTPS, any client pointing at `http://center` breaks. The edgion-cli
client (`EdgionClient`, `src/core/ctl/cli/client.rs:52-66`) builds its `reqwest` client with
**no** TLS-verify knob and defaults `base_url` to `http://localhost:PORT`. Note the existing
`ssl_verify` field (`src/core/common/auth/config.rs:64`) governs the **OIDC JWKS discovery
fetch**, *not* the CLI→Center connection — it is **not** a reusable knob here. So flipping
Center to HTTPS requires both a scheme change **and adding a new cert-verify config to the CLI**.
This is **out of scope for this task** (the source task flags the client impact at line 61) but
recorded so the follow-up is not silent: it must land before any deployment makes Center
HTTPS-only, and it is more work than "reuse an existing flag."

## Out of Scope

- mTLS / client-certificate verification (`ca` field) — dropped, possible future task.
- Controller HTTPS — separate task `admin-api-03-controller-https.md`, reuses
  `AdminTlsConfig`.
- TLS for the probe and metrics listeners — they stay plain HTTP by design. **Recorded
  residual exposure**: the probe (`:12200`) and metrics (`:12290`) sockets bind `0.0.0.0`,
  carry no auth, and run their own `axum::serve` outside `compose_admin_routes`
  (`center/cli/mod.rs:256-280`). They are therefore covered by neither this task's `admin_tls`
  nor task 02's IP allowlist (which lives inside `compose_admin_routes`). Accepted here as
  cluster-internal info-disclosure surface (metrics topology/throughput leakage), but noted so
  it is a conscious deferral, not an assumed-covered surface. Caveat: the sockets bind
  `0.0.0.0`, so "cluster-internal" holds only as long as no `Service` / LoadBalancer fronts
  `:12200` / `:12290` and a NetworkPolicy is in place — a misconfigured all-ports LoadBalancer
  would make them externally reachable. Protecting those sockets (network policy / separate
  allowlist) is a future concern outside the 01/02/03 task set.
- Self-signed certificate auto-generation via `rcgen` at runtime — not part of this task.
- **In-process certificate hot-reload** — `RustlsConfig::reload_from_pem_file` exists but is
  unused anywhere in `src/`. Cert rotation (cert-manager / 90-day ACME) is handled
  operationally by pod restart (e.g. a reloader sidecar) or by the front proxy. In-process
  reload is a conscious deferral, not an oversight.
- edgion-cli HTTPS client wiring — see "Client-side impact" above.
- Default deployment manifests / Helm values that satisfy the H2 closure condition — see
  "What counts as H2 closed".

## Effort

~200–300 lines including tests, matching the original task estimate.
