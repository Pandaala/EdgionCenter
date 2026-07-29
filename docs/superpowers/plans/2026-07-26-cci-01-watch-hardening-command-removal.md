# CCI-01: Watch Hardening + Command-Channel Removal — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the federation Command channel (Reload rewired through the HTTP proxy
tunnel) and the unused `supported_kinds` register field; make the Center watch stop
retrying on terminal errors; close the named watch test gaps.

**Architecture:** The federation stream keeps exactly four flows (register/heartbeat,
watch, proxy, stats). Reload becomes an ordinary proxied `POST /api/v1/reload` authorized
by a new default-policy grant. Deleted proto tags become `reserved`. The Center watch gains
one new outcome (`Terminal`, currently only for the Controller's `"Forbidden"` error) that
stops the re-watch loop for the session instead of retrying forever.

**Tech Stack:** Rust, tonic 0.12 (Center) / tonic (Edgion), axum, tokio. Two independent
repositories: `/Volumes/ExtStore/ws5/Edgion` (branch feature-06-24) and
`/Volumes/ExtStore/ws5/EdgionCenter` (branch feature-0716).

**Spec:** `EdgionCenter/docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md` — section 7 (CCI-01 entry), section 4, and the section 1 inventory rows for `supported_kinds` and the Command channel.

## Global Constraints

- Everything written to disk must be English.
- Two independent repos: run git via `git -C <repo>`; never mix changes in one commit.
- Commits are authorized for this run (local only, NEVER push).
- The two copies of `fed_sync.proto` (Edgion `edgion-controller/src/fed_sync/proto/fed_sync.proto`, Center `crates/center-runtime/proto/fed_sync.proto`) must end **byte-identical except their two pre-existing comment differences at lines 9 and 17**. Freed tags become `reserved` at message level: `reserved 4;` in both `ControllerMessage` and `CenterMessage`, `reserved 5;` in `RegisterRequest`.
- Terminal watch error = the Controller error string `"Forbidden"` (exact match). Everything else keeps the existing 3 s backoff re-watch.
- Keep changes minimal; both repos are pre-release — delete freely, no compatibility shims, no deprecated re-exports.
- The Center reload endpoint keeps its public path (`POST /api/v1/controllers/{id}/reload`), its authz classification (`CONTROLLERS_WRITE` / `Execute`), and its dashboard caller unchanged — only the transport behind the handler changes.

---

### Task 1: Edgion — remove Command channel + `supported_kinds` from proto and client

**Files:**
- Modify: `edgion-controller/src/fed_sync/proto/fed_sync.proto` (Command messages 91-111, oneof entries at 44 and 76, `supported_kinds` at 57)
- Modify: `edgion-controller/src/fed_sync/fed_client/mod.rs` (doc lines 6-7, import line 22, Command arm 498-531, `supported_kinds` plumbing 123-130, 236, 273-279, 308-318)
- Modify: `edgion-controller/src/cli/mod.rs` (kinds computation 737-743, 777, 784, 788)
- Test: `edgion-controller/src/fed_sync/fed_client/mod.rs` `mod tests`

**Interfaces:**
- Produces: `fed_client::run(config, shutdown, admin_router, conf_mgr, authorizer)` — the `supported_kinds: Vec<String>` parameter is gone. `CenterPayload` oneof without `Command`; `CtrlPayload` without `CommandResponse`; `RegisterRequest` without `supported_kinds`.

- [ ] **Step 1: Proto edits**

In `edgion-controller/src/fed_sync/proto/fed_sync.proto`:
- Delete messages `CommandRequest` (91-98), `ReloadCommand` (100), `ApplyCommand` (102-105), `DeleteCommand` (107-111), `CommandResponse` (64-68).
- Delete oneof entries `CommandResponse command_response = 4;` (line 44) and `CommandRequest command = 4;` (line 76); add `reserved 4;` inside both `ControllerMessage` and `CenterMessage` (message level, after the oneof block).
- Delete `repeated string supported_kinds = 5;` (line 57); add `reserved 5;` inside `RegisterRequest`.

- [ ] **Step 2: Remove the Command arm and imports in fed_client**

In `edgion-controller/src/fed_sync/fed_client/mod.rs`:
- Delete the `CenterPayload::Command(cmd)` match arm (498-531). The `match payload` has no catch-all and must remain exhaustive with the four surviving arms (RegisterAck, Ping, HttpProxy, WatchRequest).
- Remove `CommandResponse` from the import at line 22; update the module doc line 7 to `//! 4. Loop: handle CenterMessage (Ping, RegisterAck, HttpProxyRequest, WatchRequest)`.

- [ ] **Step 3: Remove `supported_kinds` plumbing**

- `fed_client/mod.rs`: drop the `supported_kinds: Vec<String>` parameter from `run` (125) and `supported_kinds: &[String]` from `connect_and_run` (275); drop `supported_kinds: supported_kinds.to_vec(),` from the register build (315); update the call at 236.
- `cli/mod.rs`: delete the `kinds` computation (737-743), `kinds_for_spawn` (777), the clone (784), and the argument in the `fed_client::run(...)` call (788). Do NOT touch the unrelated `all_kinds()` users in conf_sync/grpc_server/debug_handlers (that is the separate Controller↔Gateway `GetServerInfo.supported_kinds`).

- [ ] **Step 4: Add the proxied-reload tunnel test**

In `mod tests`, next to the existing proxy tests, using the existing `run_http_proxy` helper (lines 1162-1201 — mirror its setup exactly; adjust only what the helper's signature requires):

```rust
    /// Reload now arrives as an ordinary proxied POST. The tunnel must let
    /// `POST /api/v1/reload` through the path allowlist and reach the router.
    #[tokio::test]
    async fn http_proxy_reload_post_reaches_router() {
        let admin_router = axum::Router::new().route(
            "/api/v1/reload",
            axum::routing::post(|| async { "reload-ok" }),
        );
        let resp = run_http_proxy(admin_router, "POST", "/api/v1/reload", Vec::new()).await;
        assert_eq!(resp.status_code, 200);
        assert_eq!(String::from_utf8(resp.body).expect("utf8"), "reload-ok");
    }
```

If `run_http_proxy`'s parameters differ (e.g. it takes headers or a full request struct), adapt the call site to the helper — do not change the helper or production code for this test.

- [ ] **Step 5: Build and test**

Run: `cargo check -p edgion-controller` then `cargo test -p edgion-controller --lib fed_sync::fed_client`
Expected: clean compile (the deleted oneof variants surface any missed reference as a compile error — fix by deletion, never by re-adding proto fields); all tests pass including the new one.

- [ ] **Step 6: Commit**

```bash
git -C /Volumes/ExtStore/ws5/Edgion add edgion-controller/src/fed_sync/proto/fed_sync.proto edgion-controller/src/fed_sync/fed_client/mod.rs edgion-controller/src/cli/mod.rs
git -C /Volumes/ExtStore/ws5/Edgion commit -m "refactor(fed_sync): remove the federation Command channel and supported_kinds

Reload arrives as a proxied POST /api/v1/reload; Apply/Delete were never
constructed. supported_kinds had no consumer (the Center watch is pinned
to EdgionConfigData). Freed proto tags are reserved.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Edgion — default-policy `reload` grant + policy docs

**Files:**
- Modify: `edgion-controller/src/fed_sync/default_policy.rs` (rule list 132-164, module doc line 20, fn doc 111-128, test 276-280)
- Modify: `config/edgion-controller.yaml` (default-policy comment block 166-203)
- Modify: `docs/en/ops-guide/federation-rbac.md` (table row 39, bullet 54-55)
- Modify: `skills/01-architecture/01-controller/11-authentication-authorization.md` (bullet 106)

**Interfaces:**
- Produces: `default_center_policy()` grants `Verb::Reload` on `NON_RESOURCE_KIND` (Rule 5). The Center's proxied `POST /api/v1/reload` (classified `Verb::Reload`/`NonResource` by `authz_classifier.rs:60`) passes under the default policy.

- [ ] **Step 1: Flip the policy tests first (failing)**

In `default_policy.rs` tests, replace `default_denies_reload_any` (276-280), mirroring the `server-info` pattern at 293-303:

```rust
    #[test]
    fn default_allows_reload_on_non_resource() {
        assert!(allows(Verb::Reload, NON_RESOURCE_KIND));
    }

    #[test]
    fn default_denies_reload_on_resource_kinds() {
        assert!(!allows(Verb::Reload, "Gateway"));
        assert!(!allows(Verb::Reload, "*"));
    }
```

Run: `cargo test -p edgion-controller --lib fed_sync::default_policy`
Expected: `default_allows_reload_on_non_resource` FAILS (denied today); the denial test passes.

- [ ] **Step 2: Add Rule 5**

In `default_center_policy()` after Rule 4 (before the closing `]` at ~162):

```rust
            // Rule 5: reload on the non-resource route. The Center's reload
            // button arrives as a proxied POST /api/v1/reload; the Command
            // channel that used to carry it is gone.
            FedAllowRule {
                verbs: vec![Verb::Reload],
                kinds: vec![NON_RESOURCE_KIND.to_string()],
            },
```

Match the exact struct/field names used by Rule 4 (157-162) — copy its shape.
Update the module doc line 20 (drop "no reload" from the denial list) and the
`default_center_policy` doc comment (111-128) to enumerate Rule 5.

- [ ] **Step 3: Run the policy tests**

Run: `cargo test -p edgion-controller --lib fed_sync::default_policy`
Expected: all pass.

- [ ] **Step 4: Update the three docs**

- `config/edgion-controller.yaml:166-203`: add reload to the granted list; also fix the two pre-existing inaccuracies found during inventory: line 178 wrongly lists `server-info` as denied (Rule 4 grants it), and 174-177 says "get, list, and failover" on RegionRoute while the code grants only list+failover. Make the comment match the code exactly.
- `docs/en/ops-guide/federation-rbac.md`: table row 39 and bullet 54-55 — reload is now granted by default on the non-resource route.
- `skills/01-architecture/01-controller/11-authentication-authorization.md:106`: remove `reload` from the "Nothing else" list.

Run: `python3 cicd/checks/validate_agent_docs.py`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git -C /Volumes/ExtStore/ws5/Edgion add edgion-controller/src/fed_sync/default_policy.rs config/edgion-controller.yaml docs/en/ops-guide/federation-rbac.md skills/01-architecture/01-controller/11-authentication-authorization.md
git -C /Volumes/ExtStore/ws5/Edgion commit -m "feat(fed_sync): grant reload in the default Center policy

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: EdgionCenter — rewire the reload handler to the proxy tunnel

**Files:**
- Modify: `crates/center-app/src/api/mod.rs` — handler `reload_controller` (953-977), imports 117-118, doc line 12
- Test: `crates/center-app/src/api/mod.rs` `mod tests`

**Interfaces:**
- Consumes: `ApiState.proxy: Arc<ProxyForwarder>` (`api/mod.rs:128`) with `forward(&self, controller_id: &str, method: String, path: String, headers: HashMap<String, String>, body: Vec<u8>) -> Result<HttpProxyResponse, (StatusCode, String)>` (`center-runtime/src/proxy.rs:62-69`; timeout/404/503/502/504 mapping built in).
- Produces: `reload_controller` no longer references `Commander` — Task 4 depends on this.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/center-app/src/api/mod.rs`, using the existing
`state_with_authz_mode` + `router` + `oneshot` harness:

```rust
    /// Reload rides the proxy tunnel now. With no live federation session the
    /// ProxyForwarder answers 404 (unknown controller), which must pass
    /// through instead of the old Commander 500/504 mapping.
    #[tokio::test]
    async fn reload_uses_proxy_tunnel_and_maps_unknown_controller_to_404() {
        use tower::ServiceExt;

        let app = router(state_with_authz_mode(AuthzMode::AllowAll, false));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/v1/controllers/cluster~c0/reload")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
```

Run: `cargo test -p edgion-center-app --lib api::tests::reload_uses_proxy_tunnel_and_maps_unknown_controller_to_404`
Expected: FAIL — the Commander path returns 500 or 504, not 404.

- [ ] **Step 2: Rewrite the handler**

Replace the Commander dispatch inside `reload_controller` (keep the existing extractor
signature, controller-id `~` decoding, and response-envelope helpers exactly as the
neighboring handlers use them):

```rust
    match state
        .proxy
        .forward(
            &controller_id,
            "POST".to_string(),
            "/api/v1/reload".to_string(),
            std::collections::HashMap::new(),
            Vec::new(),
        )
        .await
    {
        Ok(resp) if (200..300).contains(&resp.status_code) => {
            // existing 200 ok_body("ok") success envelope
        }
        Ok(resp) => {
            // pass the Controller's status through when mappable, else 500;
            // body: String::from_utf8_lossy(&resp.body) in the error envelope
        }
        Err((status, message)) => {
            // (status, message) in the error envelope — this carries the
            // ProxyForwarder's 404/503/502/504 semantics unchanged
        }
    }
```

Remove the now-unused imports `command_request::Command` (117) and `ReloadCommand` (118).
Update the doc line 12 from "send reload command" to "reload via the proxy tunnel".

- [ ] **Step 3: Run the tests**

Run: `cargo test -p edgion-center-app --lib api`
Expected: the new test passes; if an existing test asserted the old Commander mapping
(grep the tests for `reload`), update its expectation to the proxy semantics.

- [ ] **Step 4: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add crates/center-app/src/api/mod.rs
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "refactor(api): reload controllers through the proxy tunnel

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: EdgionCenter — delete the Command channel

**Files:**
- Delete: `crates/center-runtime/src/commander.rs` (entire file)
- Modify: `crates/center-runtime/proto/fed_sync.proto` (same edits as Task 1 Step 1: messages 64-68/91-111, oneof entries 44/76, `reserved 4;` in both envelope messages — do NOT touch `supported_kinds` here, that is Task 5)
- Modify: `crates/center-runtime/proto/internal_forwarding.proto` (`ForwardCommand` RPC line 6, `ForwardCommandRequest` 19-25)
- Modify: `crates/center-runtime/src/lib.rs:9` (drop `pub mod commander;`)
- Modify: `crates/center-runtime/src/federation/server.rs` (import 20, field 919, init 949, clone 1313, `CommandResponse` arm 1558-1563, doc line 7)
- Modify: `crates/center-runtime/src/internal_forwarding/mod.rs` (imports 17/19, trait method 53-59, `command_error` 95-97, client `forward_command` 196-226, `commander` field 316, `command_dispatch_status` 364-373, `::new` param 388/397, server handler 459-488, tests 579-655)
- Modify: `crates/center-runtime/src/proxy.rs:456-464` and `crates/center-runtime/src/eviction.rs:208-216` (test transports drop `forward_command`)
- Modify: `crates/center-runtime/src/federation/config.rs` (doc comment only: `command_timeout_secs` now governs proxied requests; keep the field name)
- Modify: `crates/center-app/src/lib.rs:26-28` (drop the commander re-export), `crates/center-app/src/api/mod.rs:115,127` (drop the `ApiState.commander` field + import)
- Modify: every test ApiState builder that sets `commander:` — the inventoried list: `api/global_connection_ip_restriction_handlers.rs`, `api/cloudflare_dns.rs`, `api/provider_capabilities.rs`, `api/roles.rs`, `api/consistency_handlers.rs`, `api/audit.rs`, `api/provider_credential_inspections.rs`, `api/region_route_handlers.rs`, `api/users.rs`, `api/provider_accounts.rs`, `api/mod.rs` (grep `commander` to catch them all)
- Modify: `bins/edgion-center-standalone/src/cli/mod.rs` (305-311, 365) and `bins/edgion-center-standalone/src/lib.rs:3`; `bins/edgion-center-kubernetes/src/lib.rs` (import 12, 323-330, 383, 393)

**Interfaces:**
- Consumes: Task 3 (no production `Commander` caller remains).
- Produces: `InternalForwardingService::new` without the commander parameter; `FederationGrpcServer` without `pending_commands`; `ApiState` without `commander`.

- [ ] **Step 1: Delete and fix by compiler**

Apply all deletions above, then iterate `cargo check --workspace --all-targets` until
clean — every residual reference is a compile error; resolve each by deletion (never by
re-adding a stub). Rules:
- `internal_forwarding`: keep everything shared with `ForwardHttp` (`ForwardErrorKind`,
  `ForwardError`, `OwnerForwarding`, `proxy_error`, `record_forward`, `sanitize_headers`,
  validation helpers); delete only the command-specific items listed.
- The `record_forward("command", ...)` metric call sites die with `commander.rs`; the
  metric name and its `"proxy"` operations survive untouched.
- Grep `docs/ skills/` for `ForwardCommand` / `CommandRequest` and update any hit (the
  inventory found none in Edgion; check Center's docs the same way).

- [ ] **Step 2: Run the affected test suites**

Run: `cargo test -p edgion-center-runtime -p edgion-center-app -p edgion-center-standalone`
Expected: all pass; the deleted commander/internal-forwarding command tests are gone, the
surviving internal-forwarding proxy/evict tests still pass.

- [ ] **Step 3: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add -A
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "refactor(federation): delete the Command channel

Reload rides the proxy tunnel; Apply/Delete were never constructed.
Freed fed_sync oneof tags are reserved; ForwardCommand is removed from
internal forwarding.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: EdgionCenter — delete `supported_kinds`

**Files:**
- Modify: `crates/center-runtime/proto/fed_sync.proto` (field 57 → `reserved 5;` in `RegisterRequest`)
- Modify: `crates/center-runtime/src/federation/server.rs` (validation call 595, supported_kinds branches in `validate_string_list` usage — keep `validate_string_list` itself if `environments`/`tags` use it; delete only the supported_kinds call; log field at 1026; test `validate_rejects_too_many_supported_kinds` 2197-2202; `ok_req()` 2109-2118 and literals at 2653)
- Modify: `crates/center-runtime/src/federation/registry.rs:394` (`mock_info`), plus the empty-vec literals in `global_resources.rs:1597` and `global_resource_planner.rs:890`
- Verify: both proto copies now byte-identical except the two known comment lines

- [ ] **Step 1: Delete and fix by compiler**

Apply the deletions; `cargo check -p edgion-center-runtime --all-targets` until clean.
Delete the `validate_rejects_too_many_supported_kinds` test outright (its subject is gone).

- [ ] **Step 2: Proto alignment check**

Run: `diff /Volumes/ExtStore/ws5/Edgion/edgion-controller/src/fed_sync/proto/fed_sync.proto /Volumes/ExtStore/ws5/EdgionCenter/crates/center-runtime/proto/fed_sync.proto`
Expected: exactly the two pre-existing comment-line differences (lines 9 and 17 regions), nothing else. If anything else differs, align the Center copy to the Edgion copy.

- [ ] **Step 3: Test and commit**

Run: `cargo test -p edgion-center-runtime --lib federation`
Expected: all pass.

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add crates/center-runtime
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "refactor(federation): drop the unused supported_kinds register field

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: EdgionCenter — terminal watch errors + `from_version` resume test

**Files:**
- Modify: `crates/center-runtime/src/federation/server.rs` — `WatchOutcome` (699-711), error branch of `apply_watch_event` (798-822), stream-loop match (1591-1634), initial watch request construction (1286-1301), `FedWatchState` (657-693)
- Modify: `crates/center-runtime/src/observe/fed_metrics.rs` — `watch_error_reason` labels (95-100) + drift test (527-535)
- Test: `crates/center-runtime/src/federation/server.rs` `mod tests` (helpers at 2252-2306)

**Interfaces:**
- Consumes: existing `record_watch_error(kind, reason)` (`fed_metrics.rs:181-185`).
- Produces: `WatchOutcome::Terminal`; `watch_error_reason::TERMINAL = "terminal"`; `fn initial_watch_request(request_id: &str, from_version: u64) -> CenterMessage` used by the session setup.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `server.rs`, using the existing `make_pm_cache` / `event_json`
helpers (mirror `apply_watch_event_error_backoff_rewatch` at 2547-2569 for setup):

```rust
    #[test]
    fn apply_watch_event_forbidden_is_terminal() {
        // Same setup as apply_watch_event_error_backoff_rewatch, but the
        // error string is the Controller's RBAC denial marker.
        // resp.error = "Forbidden" => WatchOutcome::Terminal, and a second
        // Forbidden frame after the state is terminated is Skipped.
    }

    #[test]
    fn terminal_watch_state_never_rewatches() {
        // After FedWatchState::terminate(), is_terminated() is true and the
        // stream loop contract is: no re_watch message is minted.
        let mut state = FedWatchState::new("r1".to_string(), None);
        state.terminate();
        assert!(state.is_terminated());
    }

    #[test]
    fn initial_watch_request_resumes_from_cached_version() {
        let msg = initial_watch_request("req-1", 42);
        let Some(CenterPayload::WatchRequest(req)) = msg.payload else {
            panic!("expected WatchRequest");
        };
        assert_eq!(req.request_id, "req-1");
        assert_eq!(req.kind, PLUGIN_METADATA_KIND);
        assert_eq!(req.from_version, 42);
    }
```

Fill the first test's body concretely from the mirrored test (it builds a
`FedWatchEventResponse { request_id, error: "Forbidden".into(), .. }` and asserts the
outcome). Run the three; all FAIL (missing variant/methods/function).

> Update 2026-07-28: the snippets above predate
> `tasks/new_work/18-watch-kind-constant-doubles-as-metric-label.md`. In current source
> `PLUGIN_METADATA_KIND` is `EDGION_CONFIG_DATA`, `FedWatchState::new` takes the kind first
> (`FedWatchState::new(EDGION_CONFIG_DATA, "r1".to_string(), None)`), and `re_watch()` takes
> no argument. Adapt the snippets rather than copying them verbatim.

- [ ] **Step 2: Implement**

- `WatchOutcome`: add `Terminal` variant.
- `FedWatchState`: add `terminated: bool` (default false), `fn terminate(&mut self)`,
  `fn is_terminated(&self) -> bool`. `re_watch` on a terminated state is a programming
  error — `debug_assert!(!self.terminated)`.
- `apply_watch_event` error branch (798-822): when `resp.error == "Forbidden"`, call
  `record_watch_error(kind, watch_error_reason::TERMINAL)`, log one WARN
  (`"federation watch terminated by Controller RBAC; re-register or fix center.rbac to restore the watch"`,
  with controller id), mark the state terminated, and return `WatchOutcome::Terminal`.
  If the state is already terminated, return `WatchOutcome::Skipped` before any other
  handling (place the check next to the stale-request_id skip). All other error strings
  keep the existing recv_error + `BackoffReWatch` path.
- Stream loop (1591-1634): `WatchOutcome::Terminal => {}` — no re-watch, no sleep; the
  arm exists so the match stays explicit about the contract.
- Extract the initial watch request construction (1286-1301) into
  `fn initial_watch_request(request_id: &str, from_version: u64) -> CenterMessage` and
  call it from the session setup with `pm_cache.get_sync_version()`.
- `fed_metrics.rs`: add `pub const TERMINAL: &str = "terminal";` to `watch_error_reason`
  and add `"terminal"` to the drift test's expected set (527-535).

- [ ] **Step 3: Run the tests**

Run: `cargo test -p edgion-center-runtime --lib federation::server` and
`cargo test -p edgion-center-runtime --lib observe::fed_metrics`
Expected: all pass, including the untouched backoff test (non-Forbidden errors keep
retrying).

- [ ] **Step 4: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add crates/center-runtime/src/federation/server.rs crates/center-runtime/src/observe/fed_metrics.rs
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "feat(federation): stop the watch on terminal Forbidden errors

A Controller RBAC denial re-watched every 3s forever. Forbidden now
terminates the session's watch (metric reason \"terminal\"); transient
errors keep the backoff re-watch. Initial watch construction is
extracted and the from_version resume path is under test.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Repository-wide verification

**Repos:** both

- [ ] **Step 1: Edgion full check**

Working directory `/Volumes/ExtStore/ws5/Edgion`:
`cargo fmt --all` && `cargo check --workspace --all-targets` && `cargo clippy --workspace --all-targets` && `python3 cicd/checks/validate_agent_docs.py`
Expected: clean. If fmt/clippy need trivial fixes in the changed files, commit them as `chore: appease fmt/clippy for cci-01`.

- [ ] **Step 2: EdgionCenter full check**

Working directory `/Volumes/ExtStore/ws5/EdgionCenter`:
`cargo fmt --all` && `cargo check --workspace --all-targets` && `cargo clippy --workspace --all-targets` && `cargo test -p edgion-center-app -p edgion-center-runtime -p edgion-center-standalone -p edgion-center-kubernetes`
Expected: clean, except the 4 pre-existing `global_resource_planner` failures recorded during CCI-00 (namespace `"edgion-data"` fixture) — those are out of scope; report them unchanged. Any NEW failure must be fixed or escalated.

- [ ] **Step 3: Final proto alignment + report**

Re-run the proto `diff` from Task 5 Step 2 (expected: only the two comment lines).
Report per repository: files changed, checks run with results, deviations. Do not push.
