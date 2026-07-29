# CCI-00: Federation Transport Prerequisite Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the four transport-layer fixes (F1, F2, F3, F5 from the write-model
convergence spec section 2) that the unified write model depends on.

**Architecture:** Four independent, minimal fixes on the existing federation proxy path:
one Controller-side header-allowlist addition (Edgion repo), gRPC/HTTP size-limit
alignment and a timeout default change on the Center side, and a percent-decode fix in the
Center audit middleware. No new components, no behavior redesign.

**Tech Stack:** Rust, tonic 0.12, axum, tokio. Two independent git repositories:
`/Volumes/ExtStore/ws5/Edgion` and `/Volumes/ExtStore/ws5/EdgionCenter`.

**Spec:** `EdgionCenter/docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md` (section 2; F4 is deliberately NOT here — it ships with CCI-05).

## Global Constraints

- Everything written to disk (code, comments, tests, commit messages) must be English.
- Two independent repos: always run git via `git -C /Volumes/ExtStore/ws5/Edgion` or
  `git -C /Volumes/ExtStore/ws5/EdgionCenter`; never mix changes in one commit.
- Repo policy forbids autonomous commits. Execute the commit steps only if the user has
  authorized commits for this plan run; otherwise report the staged diff and skip.
- Exact values (verbatim from the spec): allowlist gains `if-match`; Center federation
  gRPC decode limit 16 MiB; Center proxy route body limit 1 MiB; `command_timeout_secs`
  default 25.
- Keep changes minimal; no unrelated refactors, no compatibility shims (both repos are
  pre-release).

---

### Task 1: F1 — Forward `if-match` through the Controller federation proxy

**Repo:** Edgion

**Files:**
- Modify: `edgion-controller/src/fed_sync/fed_client/mod.rs:40-44` (const + doc comment)
- Test: same file, `mod tests` (starts at `edgion-controller/src/fed_sync/fed_client/mod.rs:918`)

**Interfaces:**
- Consumes: `handle_center_message(payload, tx, admin_router, conf_mgr, &mut active_watch_cancel, proxy_semaphore, authorizer)` — signature as used by the existing test `watch_request_denied_by_rbac_sends_forbidden_and_no_watch_started` (same file, ~line 1012). Mirror its setup exactly.
- Produces: `ALLOWED_PROXY_HEADERS` containing `"if-match"` — the Center's CAS
  preconditions (CCI-08) rely on this reaching the admin router.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `edgion-controller/src/fed_sync/fed_client/mod.rs`, next to the
existing `watch_request_denied_by_rbac_sends_forbidden_and_no_watch_started` test:

```rust
    /// The federation proxy must forward `If-Match` (CAS precondition for
    /// Center-issued writes; DELETE has no body fallback) and must keep
    /// stripping credential headers such as `Authorization`.
    #[tokio::test]
    async fn http_proxy_forwards_if_match_and_strips_authorization() {
        use crate::authz::{Authorizer, CenterPolicy};
        use crate::conf_mgr::{ConfCenterConfig, ConfMgr, FileSystemConfig};
        use crate::fed_sync::proto::{controller_message::Payload as CtrlPayload, HttpProxyRequest};
        use std::collections::HashMap;
        use std::sync::Arc;

        let tmp = tempfile::tempdir().expect("tempdir");
        let conf_mgr = Arc::new(
            ConfMgr::create(ConfCenterConfig::FileSystem(FileSystemConfig::new(
                tmp.path().to_path_buf(),
            )))
            .await
            .expect("ConfMgr::create failed"),
        );
        let authorizer = Arc::new(Authorizer::new(CenterPolicy::deny_all()));
        let (tx, mut rx) = tokio::sync::mpsc::channel::<ControllerMessage>(8);
        let tx = Arc::new(tx);
        let admin_router = axum::Router::new().route(
            "/api/v1/echo-headers",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                let if_match = headers
                    .get("if-match")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let has_authorization = headers.contains_key("authorization");
                format!("if-match={if_match};authorization-present={has_authorization}")
            }),
        );
        let proxy_semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let mut active_watch_cancel: Option<tokio_util::sync::CancellationToken> = None;

        let payload = CenterPayload::HttpProxy(HttpProxyRequest {
            request_id: "p1".to_string(),
            method: "GET".to_string(),
            path: "/api/v1/echo-headers".to_string(),
            headers: HashMap::from([
                ("if-match".to_string(), "\"v123\"".to_string()),
                ("authorization".to_string(), "Bearer secret".to_string()),
            ]),
            body: Vec::new(),
        });

        handle_center_message(
            payload,
            tx,
            admin_router,
            conf_mgr,
            &mut active_watch_cancel,
            proxy_semaphore,
            authorizer,
        )
        .await
        .expect("handle_center_message failed");

        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for HttpProxyResponse")
            .expect("channel closed without a response");
        let Some(CtrlPayload::HttpProxyResponse(resp)) = msg.payload else {
            panic!("expected HttpProxyResponse, got {:?}", msg.payload);
        };
        assert_eq!(resp.status_code, 200);
        let body = String::from_utf8(resp.body).expect("utf8 body");
        assert_eq!(body, "if-match=\"v123\";authorization-present=false");
    }
```

If any import or field name does not compile, align it with the neighboring
`watch_request_denied_by_rbac_sends_forbidden_and_no_watch_started` test — that test is
the authoritative template for this harness. Do not change production code to make the
test compile.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p edgion-controller --lib fed_sync::fed_client::tests::http_proxy_forwards_if_match_and_strips_authorization -- --nocapture`
(working directory `/Volumes/ExtStore/ws5/Edgion`)

Expected: FAIL on the body assertion with actual `if-match=;authorization-present=false`
(the allowlist currently drops `if-match`).

- [ ] **Step 3: Add `if-match` to the allowlist**

In `edgion-controller/src/fed_sync/fed_client/mod.rs`, change the const (currently line 44)
and extend its doc comment:

```rust
/// Request headers forwarded from a Center HttpProxy request into the admin
/// router. Everything else (hop-by-hop, `Authorization`, `Cookie`,
/// `X-Forwarded-*`, ...) is dropped: the Center identity is established by mTLS
/// plus the injected RBAC role, never by a Center-supplied request header.
/// `if-match` is forwarded because it carries the CAS precondition for
/// Center-issued writes; DELETE has no request-body fallback for it.
const ALLOWED_PROXY_HEADERS: &[&str] = &["accept", "content-type", "if-match", "user-agent"];
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p edgion-controller --lib fed_sync::fed_client::tests::http_proxy_forwards_if_match_and_strips_authorization`
Expected: PASS

- [ ] **Step 5: Run the whole fed_client test module**

Run: `cargo test -p edgion-controller --lib fed_sync::fed_client`
Expected: all PASS (no existing test asserts the old three-element allowlist; if one does,
update its expectation to include `if-match`).

- [ ] **Step 6: Commit (only if commits are authorized)**

```bash
git -C /Volumes/ExtStore/ws5/Edgion add edgion-controller/src/fed_sync/fed_client/mod.rs
git -C /Volumes/ExtStore/ws5/Edgion commit -m "fix(fed_sync): forward if-match through the federation proxy allowlist

The Center sends If-Match as the CAS precondition on proxied writes.
PUT survived via the body resourceVersion fallback, but DELETE carries
an empty body, so dropping If-Match made federation deletes
unconditional.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: F2a — Raise the Center federation gRPC decode limit to 16 MiB

**Repo:** EdgionCenter

**Files:**
- Modify: `crates/center-runtime/src/federation/server.rs` (new pub const near the existing consts around line 42, plus a unit test in its `mod tests`)
- Modify: `bins/edgion-center-standalone/src/cli/mod.rs:496`
- Modify: `bins/edgion-center-kubernetes/src/lib.rs:488-490`

**Interfaces:**
- Produces: `edgion_center_runtime::federation::server::MAX_FED_DECODE_MESSAGE_BYTES: usize` — both binaries reference it.

- [ ] **Step 1: Add the shared const with a pinning test**

In `crates/center-runtime/src/federation/server.rs`, near the other module consts:

```rust
/// Upper bound for decoding one `ControllerMessage` from the federation
/// stream. Must exceed the Controller's 10 MiB proxied-response body cap
/// (plus envelope overhead): tonic's 4 MiB default tears down the entire
/// federation stream — heartbeat and watch included — on a single
/// oversized frame instead of failing only that request.
pub const MAX_FED_DECODE_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
```

In the same file's `mod tests`:

```rust
    #[test]
    fn fed_decode_limit_covers_controller_proxy_response_cap() {
        // Controller-side cap: 10 MiB response body (fed_client MAX body read),
        // plus headroom for headers/envelope. See the spec, section 2 (F2).
        const CONTROLLER_PROXY_RESPONSE_CAP: usize = 10 * 1024 * 1024;
        assert!(MAX_FED_DECODE_MESSAGE_BYTES >= CONTROLLER_PROXY_RESPONSE_CAP + 64 * 1024);
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p edgion-center-runtime --lib federation::server::tests::fed_decode_limit_covers_controller_proxy_response_cap`
(working directory `/Volumes/ExtStore/ws5/EdgionCenter`)
Expected: PASS

- [ ] **Step 3: Wire the limit into the standalone binary**

`bins/edgion-center-standalone/src/cli/mod.rs` (currently line 496):

```rust
        let grpc_handle = tokio::spawn(
            server_builder
                .add_service(FederationSyncServer::new(grpc_server).max_decoding_message_size(
                    edgion_center_runtime::federation::server::MAX_FED_DECODE_MESSAGE_BYTES,
                ))
                .serve(grpc_addr),
        );
```

- [ ] **Step 4: Wire the limit into the Kubernetes binary**

`bins/edgion-center-kubernetes/src/lib.rs` (currently lines 488-490):

```rust
        .add_service(
            common::fed_sync::proto::federation_sync_server::FederationSyncServer::new(grpc_server)
                .max_decoding_message_size(
                    edgion_center_runtime::federation::server::MAX_FED_DECODE_MESSAGE_BYTES,
                ),
        );
```

- [ ] **Step 5: Compile both binaries**

Run: `cargo check -p edgion-center-standalone -p edgion-center-kubernetes`
Expected: clean. (End-to-end verification of a >4 MiB frame is a CCI-10 integration case,
per the spec's validation section.)

- [ ] **Step 6: Commit (only if commits are authorized)**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add crates/center-runtime/src/federation/server.rs bins/edgion-center-standalone/src/cli/mod.rs bins/edgion-center-kubernetes/src/lib.rs
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "fix(federation): raise gRPC decode limit above the controller response cap

tonic's 4 MiB default is below the Controller's 10 MiB proxied-response
cap, so one oversized list response killed the whole federation stream.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: F2b — Cap the Center proxy route request body at 1 MiB

**Repo:** EdgionCenter

**Files:**
- Modify: `crates/center-app/src/api/mod.rs:393` (proxy route)
- Test: `crates/center-app/src/api/mod.rs`, `mod tests` (starts at line 1276; uses the existing `state_with_authz_mode` + `router(...)` + `oneshot` harness)

**Interfaces:**
- Consumes: `state_with_authz_mode(AuthzMode::AllowAll, false)` and `router(state)` — as used by the existing test `unavailable_management_capabilities_do_not_mount_routes` (~line 1813).

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/center-app/src/api/mod.rs`:

```rust
    /// The proxy route must reject request bodies above the Controller's
    /// 1 MiB federation cap locally, instead of letting axum's 2 MiB default
    /// through only for the Controller to 413 it after a full tunnel trip.
    #[tokio::test]
    async fn proxy_route_rejects_body_over_controller_limit() {
        use tower::ServiceExt;

        let app = router(state_with_authz_mode(AuthzMode::AllowAll, false));
        let oversized = vec![b'x'; 1024 * 1024 + 1];
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("PUT")
                    .uri("/api/v1/proxy/cluster~c0/api/v1/namespaced/edgionconfigdata/ns/name")
                    .header("content-type", "application/yaml")
                    .body(axum::body::Body::from(oversized))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p edgion-center-app --lib api::tests::proxy_route_rejects_body_over_controller_limit`
Expected: FAIL — the status is not 413 (the 1 MiB+1 body passes axum's 2 MiB default and
reaches the handler, which fails on the unknown controller instead).

- [ ] **Step 3: Add the body limit to the proxy route**

`crates/center-app/src/api/mod.rs` (currently line 393). Follow the file's existing
`DefaultBodyLimit` idiom (e.g. lines 519, 635):

```rust
        // HTTP proxy to controllers. Body cap mirrors the Controller-side
        // 1 MiB federation proxy limit so oversized writes fail locally.
        .route(
            "/api/v1/proxy/{controller_id}/{*rest}",
            any(proxy_handler).layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)),
        );
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p edgion-center-app --lib api::tests::proxy_route_rejects_body_over_controller_limit`
Expected: PASS

- [ ] **Step 5: Run the api test module**

Run: `cargo test -p edgion-center-app --lib api`
Expected: all PASS

- [ ] **Step 6: Commit (only if commits are authorized)**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add crates/center-app/src/api/mod.rs
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "fix(api): cap proxy route bodies at the controller federation limit

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: F3 — Lower the Center forward-timeout default to 25 s

**Repo:** EdgionCenter

**Files:**
- Modify: `crates/center-runtime/src/federation/config.rs:14`
- Modify: `bins/edgion-center-standalone/src/config/mod.rs:276` and `:486` (existing default assertions)
- Modify: sample configs that mirror the default: `config/edgion-center.yaml:26`, `cicd/deploy/center-test/deploy.yaml:112`, `cicd/deploy/examples/cloudflare-mounted-credentials/config.yaml:13`, `cicd/deploy/examples/aws-route53-ambient/config.yaml:13`, `cicd/deploy/center-kubernetes/config.yaml:17`

- [ ] **Step 1: Update the two default assertions first (failing tests)**

In `bins/edgion-center-standalone/src/config/mod.rs`, change both assertions
(currently lines 276 and 486):

```rust
        assert_eq!(config.sync.command_timeout_secs, 25);
```

(keep the trailing `// default` comment on the line that has one)

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p edgion-center-standalone --lib config`
Expected: FAIL — actual value 30.

- [ ] **Step 3: Change the default**

`crates/center-runtime/src/federation/config.rs`:

```rust
impl Default for CenterSyncConfig {
    fn default() -> Self {
        Self {
            // Below the dashboard's 30 s axios timeout so a slow Controller
            // surfaces as a Center 504, not a browser-side abort.
            command_timeout_secs: 25,
            ping_interval_secs: 30,
        }
    }
}
```

- [ ] **Step 4: Run the config tests to verify they pass**

Run: `cargo test -p edgion-center-standalone --lib config`
Expected: PASS

- [ ] **Step 5: Update the five sample configs**

Change `command_timeout_secs: 30` to `command_timeout_secs: 25` in the five YAML files
listed above, then confirm no stragglers:

Run: `grep -rn "command_timeout_secs: 30" /Volumes/ExtStore/ws5/EdgionCenter --include="*.yaml" | grep -v target`
Expected: no output.

- [ ] **Step 6: Commit (only if commits are authorized)**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add crates/center-runtime/src/federation/config.rs bins/edgion-center-standalone/src/config/mod.rs config/edgion-center.yaml cicd/deploy/center-test/deploy.yaml cicd/deploy/examples/cloudflare-mounted-credentials/config.yaml cicd/deploy/examples/aws-route53-ambient/config.yaml cicd/deploy/center-kubernetes/config.yaml
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "fix(federation): default forward timeout below the dashboard timeout

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: F5 — Percent-decode the audit target controller id

**Repo:** EdgionCenter

**Files:**
- Modify: `crates/center-app/src/common/audit/middleware.rs:61-74` (`parse_target_controller` + doc comment)
- Test: same file, `mod tests` (starts at line 346)

**Interfaces:**
- Consumes: `percent_encoding::percent_decode_str` (already a center-app dependency,
  `crates/center-app/Cargo.toml:47`); mirror the decode idiom in
  `crates/center-app/src/common/authz/middleware.rs:252-260`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/center-app/src/common/audit/middleware.rs`:

```rust
    #[test]
    fn parse_target_controller_decodes_percent_encoding() {
        // Percent-encoded and tilde forms must audit under the same id that
        // proxy dispatch resolves (axum percent-decodes path params).
        assert_eq!(
            parse_target_controller("/api/v1/proxy/cluster%2Fname/api/v1/x"),
            Some("cluster/name".to_string())
        );
        assert_eq!(
            parse_target_controller("/api/v1/proxy/cluster~name/api/v1/x"),
            Some("cluster/name".to_string())
        );
        // Invalid UTF-8 after decoding: record no target rather than a wrong one.
        assert_eq!(parse_target_controller("/api/v1/proxy/bad%FF/api/v1/x"), None);
        assert_eq!(parse_target_controller("/api/v1/other"), None);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p edgion-center-app --lib common::audit::middleware::tests::parse_target_controller_decodes_percent_encoding`
Expected: FAIL — first assertion returns `Some("cluster%2Fname")`.

- [ ] **Step 3: Implement the decode**

Replace `parse_target_controller` (and its doc comment) in
`crates/center-app/src/common/audit/middleware.rs`:

```rust
/// For `/api/v1/proxy/{controller_id}/...`, extract the first path segment,
/// percent-decode it (matching axum's path-param decoding in
/// `api::proxy_handler`), then map `~` -> `/`. Returns `None` for other
/// routes and for ids that do not decode to valid UTF-8 — an absent target
/// is preferable to auditing under a different string than the one
/// dispatched.
fn parse_target_controller(path: &str) -> Option<String> {
    let rest = path.strip_prefix(PROXY_PREFIX)?;
    let seg = rest.split('/').next()?;
    if seg.is_empty() {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(seg).decode_utf8().ok()?;
    Some(decoded.replace('~', "/"))
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p edgion-center-app --lib common::audit::middleware::tests::parse_target_controller_decodes_percent_encoding`
Expected: PASS

- [ ] **Step 5: Run the audit middleware test module**

Run: `cargo test -p edgion-center-app --lib common::audit`
Expected: all PASS (if an existing test asserts the raw un-decoded behavior, update it to
the decoded expectation — the old behavior is the bug).

- [ ] **Step 6: Commit (only if commits are authorized)**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add crates/center-app/src/common/audit/middleware.rs
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "fix(audit): percent-decode the proxy target controller id

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Repository-wide verification

**Repos:** both

- [ ] **Step 1: Edgion full check**

Working directory `/Volumes/ExtStore/ws5/Edgion`:

```bash
cargo fmt --all
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
python3 cicd/checks/validate_agent_docs.py
```

Expected: all clean.

- [ ] **Step 2: EdgionCenter full check**

Working directory `/Volumes/ExtStore/ws5/EdgionCenter`:

```bash
cargo fmt --all
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cargo test -p edgion-center-app -p edgion-center-runtime -p edgion-center-standalone
```

Expected: all clean / all PASS.

- [ ] **Step 3: Report**

Report per repository: files changed, checks run with results, and any deviations from
this plan. Do not push.
