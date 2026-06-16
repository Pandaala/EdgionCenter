# Refactor `sync()` into testable watch handlers — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the watch-payload logic from `FederationGrpcServer::sync()` into synchronous, unit-testable handler functions returning a `WatchOutcome`, and lift the register/SPIFFE prologue into a private method, with zero behavior change.

**Architecture:** Add `src/fed_sync/server/watch.rs` holding `FedWatchState`, `WatchEventRaw`, a new `WatchOutcome` enum, and the two pure handlers `apply_watch_list` / `apply_watch_event`. The `select!` loop in `mod.rs` matches a payload, calls a handler, and executes the side effects (`re_watch` + `send`, backoff) the returned `WatchOutcome` requests. All async/channel/timing stays in the loop; handlers are synchronous and directly testable.

**Tech Stack:** Rust, tonic/prost (proto types), tokio (loop only), serde_json, `CenterWatchCache<PluginMetaData>`, the `metrics` crate facade (no-op in tests).

---

## Reference facts (verified against the codebase)

- Proto payload variants carry generated structs: `CtrlPayload::WatchListResponse(FedWatchListResponse)` and `CtrlPayload::WatchEventResponse(FedWatchEventResponse)`. Both are plain prost structs with public fields, constructed by field literal.
  - `FedWatchListResponse { request_id: String, data: String, sync_version: u64, server_id: String }`
  - `FedWatchEventResponse { request_id, data, sync_version, server_id, error: String }`
- `CenterWatchCache::<PluginMetaData>::new(controller_id: String, handler: Arc<dyn CenterConfHandler<PluginMetaData> + Send + Sync>)` — `cache.rs:32`.
- The `MockHandler` in `watch_cache/cache.rs` is private to that file's `#[cfg(test)]` module — **tests in `watch.rs` define their own no-op handler** (the `CenterConfHandler` trait is `pub` in `watch_cache/traits.rs`).
- Build a `PluginMetaData` in tests via `PluginMetaData::new(name, PluginMetaDataSpec { metadata: MetaDataEntry::KeyList(...) })` (pattern from `common/metadata_conf_handler.rs:78`). `pm.key_name()` comes from the `ResourceMeta` trait (`edgion_resources::resource::meta::ResourceMeta`) and equals `name` when namespace is `None`.
- `fed_metrics::record_*` are safe to call in unit tests (no-op without a metrics subscriber).
- `PLUGIN_METADATA_KIND` is a private const in `server/mod.rs`. `watch.rs` is a child module, so `super::PLUGIN_METADATA_KIND` is reachable without changing its visibility.
- **Visibility:** after extraction, `mod.rs` only calls `FedWatchState::new(...)` and `pm_watch.re_watch(KIND)` — it does NOT read the fields directly. So the struct fields stay private; only the type, its `new`/`re_watch` methods, `WatchOutcome`, and the two handler fns need `pub(crate)`. `WatchEventRaw` stays fully private (used only inside `watch.rs`).
- `apply_watch_list` only ever returns `Skip` or `Applied` (no re-watch path). Only `apply_watch_event` returns `ReWatch` / `BackoffThenReWatch`. The loop's `WatchListResponse` arm therefore needs no outcome handling.

---

## Task 1: Create `watch.rs`, move types, add `WatchOutcome`

**Files:**
- Create: `src/fed_sync/server/watch.rs`
- Modify: `src/fed_sync/server/mod.rs` (remove moved defs lines 133-183; add `mod watch;` + imports)

- [ ] **Step 1: Create `src/fed_sync/server/watch.rs` with the moved types**

Cut `WatchEventRaw` (current `mod.rs:133-147`) and `FedWatchState` + its impl (current `mod.rs:149-183`) out of `mod.rs` and paste them here, adding the `WatchOutcome` enum and module imports. Adjust visibility as noted.

```rust
//! Synchronous, unit-testable handlers for the federation watch payloads.
//!
//! The `sync()` message loop matches a payload, calls a handler here, and acts
//! on the returned [`WatchOutcome`]. Handlers do parse + cache apply + state
//! update + metric/log emission only — they never touch the channel, sleep, or
//! call `re_watch`. That keeps all async/timing concerns in the loop and leaves
//! this module synchronously testable.

use uuid::Uuid;

use crate::common::conf_sync::types::EventType;
use crate::common::fed_sync::proto::{
    center_message::Payload as CenterPayload, CenterMessage, FedWatchEventResponse, FedWatchListResponse,
    FedWatchRequest,
};
use crate::common::observe::fed_metrics;
use crate::watch_cache::{CenterWatchCache, WatchEventSimple};
use edgion_resources::resource::meta::ResourceMeta;
use edgion_resources::resources::plugin_metadata::PluginMetaData;

/// What the `sync()` loop should do after a handler processed a watch payload.
///
/// `re_watch` (request_id rotation + `consecutive_errors` reset) and any
/// channel/sleep work are performed by the loop, not the handler.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WatchOutcome {
    /// Payload handled; loop does nothing further.
    Applied,
    /// Stale `request_id`; loop skips (equivalent to today's `continue`).
    Skip,
    /// Loop should `re_watch` immediately and send the request.
    ReWatch,
    /// Loop should back off (3s, abortable on session close), then `re_watch`.
    BackoffThenReWatch,
}

/// Typed representation of a single watch event from the controller.
///
/// `data` is borrowed as `&RawValue` so the outer batch parse only slices the
/// array; each item's payload is deserialized lazily into its concrete type,
/// avoiding an intermediate `serde_json::Value` tree per event.
#[derive(serde::Deserialize)]
struct WatchEventRaw<'a> {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(borrow)]
    data: &'a serde_json::value::RawValue,
    #[serde(default)]
    #[allow(dead_code)]
    sync_version: u64,
}

/// Per-kind watch state for a controller session.
/// Currently only PluginMetaData is watched; this struct makes it straightforward
/// to add more kinds by storing a `HashMap<kind, FedWatchState>`.
pub(crate) struct FedWatchState {
    /// Current request_id for correlation (stale responses are skipped).
    request_id: String,
    /// Controller's ConfigSyncServer instance ID (detects restarts).
    server_id: Option<String>,
    /// Consecutive error count (INFO on first, WARN on subsequent).
    consecutive_errors: u32,
}

impl FedWatchState {
    pub(crate) fn new(request_id: String, server_id: Option<String>) -> Self {
        Self {
            request_id,
            server_id,
            consecutive_errors: 0,
        }
    }

    /// Generate a new FedWatchRequest (from_version=0) and update internal request_id.
    pub(crate) fn re_watch(&mut self, kind: &str) -> CenterMessage {
        let new_id = Uuid::new_v4().to_string();
        self.request_id = new_id.clone();
        self.consecutive_errors = 0;
        CenterMessage {
            payload: Some(CenterPayload::WatchRequest(FedWatchRequest {
                request_id: new_id,
                kind: kind.to_string(),
                from_version: 0,
            })),
        }
    }
}
```

- [ ] **Step 2: Declare the module and import the types in `mod.rs`**

At the top of `src/fed_sync/server/mod.rs`, after the existing `use` block, add the module declaration and bring the needed items into scope. `WatchEventSimple` / `EventType` / `WatchEventRaw` are no longer referenced by `mod.rs` after later tasks, but `FedWatchState` and `WatchOutcome` are.

Add near the other `use` lines:

```rust
mod watch;
use watch::{FedWatchState, WatchOutcome};
```

Then delete the now-unused `use` imports in `mod.rs` that only the moved code needed — specifically `use crate::common::conf_sync::types::EventType;` and the `WatchEventSimple` part of the `watch_cache` import — **only if** the compiler flags them as unused after Task 3 (defer the deletion; leave them for now to keep Task 1 compiling).

- [ ] **Step 3: Verify the crate still builds**

Run: `cargo build`
Expected: builds successfully. `WatchOutcome` is unused so far — if `-D warnings` is in effect and `dead_code` fires, that is expected and is resolved in Task 3 when the loop matches on it; if the build fails on that warning, add a temporary `#[allow(dead_code)]` on `WatchOutcome` and remove it in Task 3.

- [ ] **Step 4: Run the existing test suite to confirm no regression**

Run: `cargo test fed_sync`
Expected: all existing `fed_sync::server` tests (validate_*, capacity_*, takeover_*) still PASS.

- [ ] **Step 5: Commit**

```bash
git add src/fed_sync/server/watch.rs src/fed_sync/server/mod.rs
git commit -m "refactor(fed-sync): move watch state types into watch.rs, add WatchOutcome"
```

---

## Task 2: `apply_watch_list` (TDD) + wire the `WatchListResponse` arm

**Files:**
- Modify: `src/fed_sync/server/watch.rs` (add `apply_watch_list` + `#[cfg(test)] mod tests`)
- Modify: `src/fed_sync/server/mod.rs` (replace the `WatchListResponse` arm, current lines 608-655)

- [ ] **Step 1: Write the failing tests**

Append to `src/fed_sync/server/watch.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use edgion_resources::resources::plugin_metadata::{
        KeyGroup, KeyListData, KeyMatchMode, MetaDataEntry, MetaDataItem, PluginMetaDataSpec,
    };

    use crate::watch_cache::traits::CenterConfHandler;

    /// No-op handler — the cache calls it on apply, but the watch tests only
    /// assert on `WatchOutcome` and cache state, not handler side effects.
    struct NoopHandler;
    impl CenterConfHandler<PluginMetaData> for NoopHandler {
        fn full_set(&self, _controller_id: &str, _data: &HashMap<String, Arc<PluginMetaData>>) {}
        fn partial_update(
            &self,
            _controller_id: &str,
            _add: HashMap<String, Arc<PluginMetaData>>,
            _update: HashMap<String, Arc<PluginMetaData>>,
            _remove: HashSet<String>,
        ) {
        }
        fn controller_offline(&self, _controller_id: &str) {}
        fn controller_removed(&self, _controller_id: &str) {}
    }

    fn cache() -> CenterWatchCache<PluginMetaData> {
        CenterWatchCache::new("ctrl-1".to_string(), Arc::new(NoopHandler))
    }

    /// Build a PluginMetaData with the given name (namespace=None ⇒ key_name == name).
    fn pm(name: &str) -> PluginMetaData {
        PluginMetaData::new(
            name,
            PluginMetaDataSpec {
                metadata: MetaDataEntry::KeyList(KeyListData {
                    match_mode: KeyMatchMode::Exact,
                    items: vec![KeyGroup {
                        name: "g".to_string(),
                        description: None,
                        items: vec![MetaDataItem {
                            key: "k".to_string(),
                            code: None,
                            priority: None,
                            id: None,
                            behavior: None,
                        }],
                    }],
                }),
            },
        )
    }

    fn list_resp(request_id: &str, server_id: &str, items: &[PluginMetaData]) -> FedWatchListResponse {
        FedWatchListResponse {
            request_id: request_id.to_string(),
            data: serde_json::to_string(items).unwrap(),
            sync_version: 7,
            server_id: server_id.to_string(),
        }
    }

    #[test]
    fn apply_watch_list_happy_path_applies_and_updates_state() {
        let cache = cache();
        let mut watch = FedWatchState::new("req-1".to_string(), None);
        let resp = list_resp("req-1", "srv-A", &[pm("a"), pm("b")]);

        let outcome = apply_watch_list("ctrl-1", &cache, &mut watch, resp);

        assert_eq!(outcome, WatchOutcome::Applied);
        assert_eq!(cache.get_sync_version(), 7);
        assert_eq!(cache.get_server_id(), "srv-A");
        assert_eq!(watch.server_id.as_deref(), Some("srv-A"));
        assert_eq!(watch.consecutive_errors, 0);
    }

    #[test]
    fn apply_watch_list_stale_request_id_is_skipped() {
        let cache = cache();
        let mut watch = FedWatchState::new("req-current".to_string(), None);
        let resp = list_resp("req-OLD", "srv-A", &[pm("a")]);

        let outcome = apply_watch_list("ctrl-1", &cache, &mut watch, resp);

        assert_eq!(outcome, WatchOutcome::Skip);
        // Cache untouched: still at initial version 0 / empty server_id.
        assert_eq!(cache.get_sync_version(), 0);
        assert_eq!(cache.get_server_id(), "");
        assert_eq!(watch.server_id, None);
    }

    #[test]
    fn apply_watch_list_parse_error_leaves_cache_untouched_and_does_not_rewatch() {
        let cache = cache();
        let mut watch = FedWatchState::new("req-1".to_string(), None);
        let resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data: "{not valid json".to_string(),
            sync_version: 7,
            server_id: "srv-A".to_string(),
        };

        let outcome = apply_watch_list("ctrl-1", &cache, &mut watch, resp);

        assert_eq!(outcome, WatchOutcome::Applied); // no-op, no re-watch
        assert_eq!(cache.get_sync_version(), 0);
        assert_eq!(watch.server_id, None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test apply_watch_list`
Expected: FAIL to compile — `cannot find function apply_watch_list in this scope`.

- [ ] **Step 3: Implement `apply_watch_list`**

Insert this function in `watch.rs` (before the `#[cfg(test)]` module). It is the current `WatchListResponse` arm logic (mod.rs:609-654) lifted verbatim, returning a `WatchOutcome`:

```rust
/// Apply a `FedWatchListResponse` (full snapshot) to the cache.
///
/// Returns `Skip` for a stale `request_id`, otherwise `Applied` (a parse failure
/// is logged + metered but is still a loop no-op — no re-watch, matching the
/// previous inline behavior).
pub(crate) fn apply_watch_list(
    cid: &str,
    cache: &CenterWatchCache<PluginMetaData>,
    watch: &mut FedWatchState,
    resp: FedWatchListResponse,
) -> WatchOutcome {
    if resp.request_id != watch.request_id {
        tracing::debug!(
            component = "fed_server",
            controller_id = %cid,
            expected = %watch.request_id,
            got = %resp.request_id,
            "Skipping stale WatchListResponse"
        );
        return WatchOutcome::Skip;
    }
    match serde_json::from_str::<Vec<PluginMetaData>>(&resp.data) {
        Ok(items) => {
            let keyed: Vec<(String, PluginMetaData)> = items
                .into_iter()
                .map(|pm| {
                    let key = pm.key_name();
                    (key, pm)
                })
                .collect();
            cache.replace_all(keyed, resp.sync_version, resp.server_id.clone());
            watch.server_id = Some(resp.server_id);
            watch.consecutive_errors = 0;
            fed_metrics::record_watch_list(
                super::PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_list_result::OK,
            );
            tracing::debug!(
                component = "fed_server",
                controller_id = %cid,
                sync_version = resp.sync_version,
                "PluginMetaData WatchListResponse applied"
            );
            WatchOutcome::Applied
        }
        Err(e) => {
            fed_metrics::record_watch_list(
                super::PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_list_result::PARSE_ERROR,
            );
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                error = %e,
                "Failed to deserialize WatchListResponse data"
            );
            WatchOutcome::Applied
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test apply_watch_list`
Expected: 3 tests PASS.

- [ ] **Step 5: Wire the `WatchListResponse` arm in the loop**

In `src/fed_sync/server/mod.rs`, replace the entire `Some(CtrlPayload::WatchListResponse(resp)) => { ... }` arm (current lines 608-655) with:

```rust
                                Some(CtrlPayload::WatchListResponse(resp)) => {
                                    // apply_watch_list only ever returns Skip/Applied — both no-ops here.
                                    let _ = watch::apply_watch_list(&cid, &pm_cache, &mut pm_watch, resp);
                                }
```

- [ ] **Step 6: Build and run the suite**

Run: `cargo build && cargo test fed_sync`
Expected: builds; all `fed_sync` tests (existing + the 3 new `apply_watch_list` tests) PASS.

- [ ] **Step 7: Commit**

```bash
git add src/fed_sync/server/watch.rs src/fed_sync/server/mod.rs
git commit -m "refactor(fed-sync): extract apply_watch_list handler with unit tests"
```

---

## Task 3: `apply_watch_event` (TDD) + wire the `WatchEventResponse` arm

**Files:**
- Modify: `src/fed_sync/server/watch.rs` (add `apply_watch_event` + tests)
- Modify: `src/fed_sync/server/mod.rs` (replace the `WatchEventResponse` arm, current lines 656-804)

- [ ] **Step 1: Write the failing tests**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `watch.rs` (reuse the `cache()`, `pm()`, `NoopHandler` helpers from Task 2). Add a `event_resp` builder and a JSON helper for the event-array shape (`[{ "type": "...", "data": <PluginMetaData>, "sync_version": N }]`):

```rust
    fn event_resp(request_id: &str, server_id: &str, error: &str, data: &str) -> FedWatchEventResponse {
        FedWatchEventResponse {
            request_id: request_id.to_string(),
            data: data.to_string(),
            sync_version: 9,
            server_id: server_id.to_string(),
            error: error.to_string(),
        }
    }

    /// Serialize one watch event entry: {"type": <t>, "data": <pm>, "sync_version": 9}.
    fn event_json(entries: &[(&str, PluginMetaData)]) -> String {
        let arr: Vec<serde_json::Value> = entries
            .iter()
            .map(|(t, pm)| {
                serde_json::json!({
                    "type": t,
                    "data": serde_json::to_value(pm).unwrap(),
                    "sync_version": 9,
                })
            })
            .collect();
        serde_json::to_string(&arr).unwrap()
    }

    #[test]
    fn apply_watch_event_stale_request_id_is_skipped() {
        let cache = cache();
        let mut watch = FedWatchState::new("req-current".to_string(), Some("srv-A".to_string()));
        let resp = event_resp("req-OLD", "srv-A", "", &event_json(&[("add", pm("a"))]));

        let outcome = apply_watch_event("ctrl-1", &cache, &mut watch, resp);

        assert_eq!(outcome, WatchOutcome::Skip);
        assert_eq!(cache.get_sync_version(), 0);
    }

    #[test]
    fn apply_watch_event_error_backs_off_and_increments_counter() {
        let cache = cache();
        let mut watch = FedWatchState::new("req-1".to_string(), Some("srv-A".to_string()));
        let resp = event_resp("req-1", "srv-A", "controller restarting", "[]");

        let outcome = apply_watch_event("ctrl-1", &cache, &mut watch, resp);

        assert_eq!(outcome, WatchOutcome::BackoffThenReWatch);
        assert_eq!(watch.consecutive_errors, 1);
    }

    #[test]
    fn apply_watch_event_server_id_change_requests_rewatch() {
        let cache = cache();
        let mut watch = FedWatchState::new("req-1".to_string(), Some("srv-OLD".to_string()));
        let resp = event_resp("req-1", "srv-NEW", "", &event_json(&[("add", pm("a"))]));

        let outcome = apply_watch_event("ctrl-1", &cache, &mut watch, resp);

        assert_eq!(outcome, WatchOutcome::ReWatch);
        // server_id mismatch short-circuits before applying events.
        assert_eq!(cache.get_sync_version(), 0);
    }

    #[test]
    fn apply_watch_event_classifies_add_update_delete_and_skips_unknown() {
        let cache = cache();
        let mut watch = FedWatchState::new("req-1".to_string(), Some("srv-A".to_string()));
        // Seed an entry to delete: first an add for "x".
        let seed = event_resp("req-1", "srv-A", "", &event_json(&[("add", pm("x"))]));
        assert_eq!(
            apply_watch_event("ctrl-1", &cache, &mut watch, seed),
            WatchOutcome::Applied
        );

        // add "a", update "x", delete "x", and an unknown type that is skipped.
        let data = event_json(&[
            ("add", pm("a")),
            ("update", pm("x")),
            ("delete", pm("x")),
            ("frobnicate", pm("ignored")),
        ]);
        let resp = event_resp("req-1", "srv-A", "", &data);

        let outcome = apply_watch_event("ctrl-1", &cache, &mut watch, resp);

        assert_eq!(outcome, WatchOutcome::Applied);
        assert_eq!(cache.get_sync_version(), 9);
        assert_eq!(cache.get_server_id(), "srv-A");
        assert_eq!(watch.consecutive_errors, 0);
    }

    #[test]
    fn apply_watch_event_parse_error_is_noop_without_rewatch() {
        let cache = cache();
        let mut watch = FedWatchState::new("req-1".to_string(), Some("srv-A".to_string()));
        let resp = event_resp("req-1", "srv-A", "", "{not an array");

        let outcome = apply_watch_event("ctrl-1", &cache, &mut watch, resp);

        assert_eq!(outcome, WatchOutcome::Applied);
        assert_eq!(cache.get_sync_version(), 0);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test apply_watch_event`
Expected: FAIL to compile — `cannot find function apply_watch_event in this scope`.

- [ ] **Step 3: Implement `apply_watch_event`**

Insert this function in `watch.rs` (after `apply_watch_list`, before the test module). It is the current `WatchEventResponse` arm (mod.rs:657-803) lifted verbatim, with the stale-skip / error / server-id branches returning the matching `WatchOutcome` and the channel/sleep/`re_watch` work removed (now the loop's job):

```rust
/// Apply a `FedWatchEventResponse` (incremental events) to the cache.
///
/// Returns:
/// - `Skip` — stale `request_id`.
/// - `BackoffThenReWatch` — non-empty `error` (counter incremented, logged INFO
///   on the first error then WARN). The loop performs the 3s abortable backoff
///   and the `re_watch` (which resets the counter).
/// - `ReWatch` — `server_id` changed (controller restart).
/// - `Applied` — events parsed and applied (or a parse failure, which is logged +
///   metered but is still a loop no-op).
pub(crate) fn apply_watch_event(
    cid: &str,
    cache: &CenterWatchCache<PluginMetaData>,
    watch: &mut FedWatchState,
    resp: FedWatchEventResponse,
) -> WatchOutcome {
    if resp.request_id != watch.request_id {
        tracing::debug!(
            component = "fed_server",
            controller_id = %cid,
            expected = %watch.request_id,
            got = %resp.request_id,
            "Skipping stale WatchEventResponse"
        );
        return WatchOutcome::Skip;
    }

    // Watch event delivered to Center (direction = recv from the Center's point of
    // view: events flow Controller → Center).
    fed_metrics::record_watch_event(
        super::PLUGIN_METADATA_KIND,
        fed_metrics::labels::direction::RECV,
    );

    // Error → backoff then re-watch from 0
    if !resp.error.is_empty() {
        fed_metrics::record_watch_error(
            super::PLUGIN_METADATA_KIND,
            fed_metrics::labels::watch_error_reason::RECV_ERROR,
        );
        watch.consecutive_errors += 1;
        if watch.consecutive_errors == 1 {
            // First error is normal during startup/reload
            tracing::info!(
                component = "fed_server",
                controller_id = %cid,
                error = %resp.error,
                "WatchEventResponse error (likely startup delay), backing off before re-watch"
            );
        } else {
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                error = %resp.error,
                consecutive_errors = watch.consecutive_errors,
                "WatchEventResponse error persists, backing off before re-watch"
            );
        }
        return WatchOutcome::BackoffThenReWatch;
    }

    // Server restart detection → re-watch from 0
    if let Some(ref expected_sid) = watch.server_id {
        if *expected_sid != resp.server_id {
            fed_metrics::record_watch_list(
                super::PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_list_result::VERSION_TOO_OLD,
            );
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                expected_server_id = %expected_sid,
                got_server_id = %resp.server_id,
                "Controller server_id changed, re-watching from 0"
            );
            return WatchOutcome::ReWatch;
        }
    }

    // Parse events using typed struct
    match serde_json::from_str::<Vec<WatchEventRaw>>(&resp.data) {
        Ok(raw_events) => {
            let mut events = Vec::new();
            for raw in raw_events {
                let event_type = match raw.event_type.as_str() {
                    "add" => EventType::Add,
                    "update" => EventType::Update,
                    "delete" => EventType::Delete,
                    other => {
                        tracing::warn!(
                            component = "fed_server",
                            controller_id = %cid,
                            event_type = other,
                            "Unknown watch event type, skipping"
                        );
                        continue;
                    }
                };
                match serde_json::from_str::<PluginMetaData>(raw.data.get()) {
                    Ok(pm) => {
                        let key = pm.key_name();
                        events.push(WatchEventSimple {
                            event_type,
                            key,
                            data: pm,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            component = "fed_server",
                            controller_id = %cid,
                            error = %e,
                            "Failed to parse watch event data as PluginMetaData"
                        );
                    }
                }
            }
            if !events.is_empty() {
                cache.apply_events(events, resp.sync_version, resp.server_id.clone());
            }
            watch.server_id = Some(resp.server_id);
            watch.consecutive_errors = 0;
            tracing::debug!(
                component = "fed_server",
                controller_id = %cid,
                sync_version = resp.sync_version,
                "PluginMetaData WatchEventResponse applied"
            );
            WatchOutcome::Applied
        }
        Err(e) => {
            fed_metrics::record_watch_error(
                super::PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_error_reason::PARSE_ERROR,
            );
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                error = %e,
                "Failed to parse WatchEventResponse data"
            );
            WatchOutcome::Applied
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test apply_watch_event`
Expected: 5 tests PASS.

- [ ] **Step 5: Wire the `WatchEventResponse` arm in the loop**

In `src/fed_sync/server/mod.rs`, replace the entire `Some(CtrlPayload::WatchEventResponse(resp)) => { ... }` arm (current lines 656-804) with the handler call + outcome execution. This is the only arm that performs the `re_watch`/backoff side effects:

```rust
                                Some(CtrlPayload::WatchEventResponse(resp)) => {
                                    match watch::apply_watch_event(&cid, &pm_cache, &mut pm_watch, resp) {
                                        WatchOutcome::Skip | WatchOutcome::Applied => {}
                                        WatchOutcome::ReWatch => {
                                            let msg = pm_watch.re_watch(PLUGIN_METADATA_KIND);
                                            let _ = inner_tx.send(msg).await;
                                        }
                                        WatchOutcome::BackoffThenReWatch => {
                                            // Backoff before retrying to avoid a tight loop;
                                            // abort if the session closes during the sleep.
                                            tokio::select! {
                                                _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {}
                                                _ = inner_tx.closed() => {
                                                    tracing::debug!(
                                                        component = "fed_server",
                                                        controller_id = %cid,
                                                        "Session closed during backoff, stopping re-watch"
                                                    );
                                                    break;
                                                }
                                            }
                                            let msg = pm_watch.re_watch(PLUGIN_METADATA_KIND);
                                            let _ = inner_tx.send(msg).await;
                                        }
                                    }
                                }
```

- [ ] **Step 6: Remove now-unused imports in `mod.rs`**

`EventType` and `WatchEventSimple` (and `WatchEventRaw`, `FedWatchRequest`, `Uuid` if no longer used by `mod.rs`) moved with the logic. Run `cargo build` and delete whichever imports the compiler reports as `unused import` in `mod.rs`. Note: `Uuid` and `FedWatchRequest` are still used by the register prologue, and `CenterPayload::WatchRequest`/`FedWatchRequest` are still used at registration — only remove what the compiler actually flags.

Run: `cargo build`
Expected: builds with no unused-import warnings.

- [ ] **Step 7: Run the full suite**

Run: `cargo test fed_sync`
Expected: existing tests + 3 `apply_watch_list` + 5 `apply_watch_event` all PASS.

- [ ] **Step 8: Commit**

```bash
git add src/fed_sync/server/watch.rs src/fed_sync/server/mod.rs
git commit -m "refactor(fed-sync): extract apply_watch_event handler, thin the select! loop"
```

---

## Task 4: Extract the register/SPIFFE prologue into `authorize_register`

**Files:**
- Modify: `src/fed_sync/server/mod.rs` (add private method; replace prologue lines 252-311 with a call)

- [ ] **Step 1: Add the `authorize_register` method**

In `impl FederationGrpcServer` (the inherent impl block ending around line 223, NOT the trait impl), add this method. It folds validate → capacity → SPIFFE (current mod.rs:252-311) and keeps every `tracing::warn!` and the `record_peer_identity_check` emission (both OK and failure paths) inside, returning only the bare `Status`. It takes the leaf DER as `Option<&[u8]>` so it does not depend on tonic's certificate type:

```rust
    /// Run all pre-state-mutation admission checks for a `RegisterRequest`:
    /// shape validation, registry capacity, and SPIFFE peer-identity binding.
    ///
    /// All rejection logging and the peer-identity metric are emitted here; the
    /// returned `Status` carries only a fixed, non-reflective message. `leaf_der`
    /// is the peer's leaf certificate in DER form (always present under mTLS;
    /// `None` is a defensive internal error).
    fn authorize_register(&self, leaf_der: Option<&[u8]>, register_req: &RegisterRequest) -> Result<(), Status> {
        // Boundary checks must run before any state mutation so a rejected request
        // leaves zero residue. Reasons are logged at warn level but the peer-facing
        // message is fixed to avoid echoing attacker input.
        if let Err(reason) = validate_register_req(register_req) {
            tracing::warn!(
                component = "fed_server",
                reason = reason,
                controller_id_len = register_req.controller_id.len(),
                cluster_len = register_req.cluster.len(),
                env_len = register_req.env.len(),
                tag_len = register_req.tag.len(),
                supported_kinds_len = register_req.supported_kinds.len(),
                "Rejected RegisterRequest: shape validation failed"
            );
            return Err(Status::invalid_argument("RegisterRequest validation failed"));
        }
        if registry_capacity_exceeded(&self.registry, &register_req.controller_id, MAX_REGISTRY_ENTRIES) {
            tracing::warn!(
                component = "fed_server",
                registry_len = self.registry.len(),
                cap = MAX_REGISTRY_ENTRIES,
                "Rejected RegisterRequest: registry at capacity"
            );
            return Err(Status::resource_exhausted("Federation registry is at capacity"));
        }

        // Peer-identity binding — always enforced (federation is mTLS-only).
        use crate::common::observe::fed_metrics::labels::peer_identity_result as pir;
        let Some(leaf) = leaf_der else {
            // Under mTLS the handshake guarantees a client cert; absence is a
            // defensive internal error, never an attacker path.
            return Err(Status::internal("missing client certificate under mTLS"));
        };
        let trust_domain = self.trust_domain.as_deref().unwrap_or_default();
        match crate::common::fed_sync::spiffe::verify(
            leaf,
            trust_domain,
            &register_req.cluster,
            &register_req.controller_id,
        ) {
            Ok(()) => fed_metrics::record_peer_identity_check(pir::OK),
            Err(e) => {
                use crate::common::fed_sync::spiffe::PeerIdentityError as E;
                let result = match e {
                    E::Mismatch => pir::MISMATCH,
                    E::NoSpiffeSan => pir::NO_SPIFFE_SAN,
                    E::MultiSan => pir::MULTI_SAN,
                    E::ParseError => pir::PARSE_ERROR,
                };
                fed_metrics::record_peer_identity_check(result);
                tracing::warn!(
                    component = "fed_server",
                    result = result,
                    controller_id_len = register_req.controller_id.len(),
                    cluster_len = register_req.cluster.len(),
                    "Rejected RegisterRequest: peer identity check failed"
                );
                return Err(Status::permission_denied("peer identity verification failed"));
            }
        }
        Ok(())
    }
```

- [ ] **Step 2: Replace the inline prologue in `sync()` with a call**

In `sync()`, replace the whole block from the `if let Err(reason) = validate_register_req(&register_req)` check through the closing brace of the peer-identity block (current mod.rs:252-311) with:

```rust
        // Admission checks (shape, capacity, SPIFFE) — all run before any state
        // mutation so a rejected request leaves zero residue.
        let leaf_der = peer_certs.as_ref().and_then(|c| c.first()).map(|c| c.as_ref());
        self.authorize_register(leaf_der, &register_req)?;
```

Leave everything from `let controller_id = register_req.controller_id.clone();` (current line 313) onward unchanged — the "Controller registered" log and `registry.register(...)` stay in `sync()`.

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: builds. If `peer_certs` is now flagged unused anywhere else, that is not expected — it is consumed by the `leaf_der` line. Fix any unused-import fallout the compiler reports.

- [ ] **Step 4: Run the suite**

Run: `cargo test fed_sync`
Expected: all `fed_sync` tests PASS (the validate_*/capacity_* unit tests still exercise the free functions directly, unaffected by the extraction).

- [ ] **Step 5: Commit**

```bash
git add src/fed_sync/server/mod.rs
git commit -m "refactor(fed-sync): lift register/SPIFFE prologue into authorize_register"
```

---

## Task 5: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full test run**

Run: `cargo test`
Expected: entire suite PASS, including the 8 new `watch` tests.

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings, no errors.

- [ ] **Step 3: Confirm the `select!` loop body shrank**

Inspect the `// 5. Main message loop` `tokio::spawn` block in `src/fed_sync/server/mod.rs`. Confirm the two watch arms are now ~3 lines (list) and ~20 lines (event-with-outcome) respectively, and the overall `select!` body is under ~120 lines.

Run (rough line count of the loop block — adjust the anchor strings if surrounding code moved):
```bash
awk '/\/\/ 5\. Main message loop/{f=1} f{print} /Ok\(Response::new/{if(f)exit}' src/fed_sync/server/mod.rs | wc -l
```
Expected: materially smaller than the original ~340-line spawn block; the `select!` body itself under ~120 lines. If it overshoots, the disconnect-cleanup arms (`Err` / `Ok(None)`) are the documented fallback extraction — but only do that if the goal is missed.

- [ ] **Step 4: Confirm no behavior drift via a targeted diff review**

Run: `git diff e310d15 -- src/fed_sync/server/mod.rs` (or `git log --oneline` to find the pre-refactor commit) and visually confirm: the metric calls, log levels/fields, the stale-session guard, `update_last_seen`, last-pong store, `mark_offline_all`, and the 3s backoff timing are all preserved — only relocated.

- [ ] **Step 5: Final commit (if Step 3/4 prompted any cleanup)**

```bash
git add -A
git commit -m "refactor(fed-sync): final cleanup after sync() handler extraction"
```

---

## Self-review notes

- **Spec coverage:** watch.rs module (Task 1); `apply_watch_list` + tests incl. happy/stale/parse-error (Task 2); `apply_watch_event` + tests incl. stale/error-backoff/server-id-change/event-classification/parse-error (Task 3); `authorize_register` prologue with logging+metrics retained and exact 252-311 boundary (Task 4); loop < ~120 lines, `cargo test` + `cargo clippy`, behavior-preservation check (Task 5). All spec acceptance criteria mapped.
- **Type consistency:** `WatchOutcome { Applied, Skip, ReWatch, BackoffThenReWatch }`, `apply_watch_list(cid, cache, watch, resp) -> WatchOutcome`, `apply_watch_event(cid, cache, watch, resp) -> WatchOutcome`, and `authorize_register(leaf_der: Option<&[u8]>, register_req) -> Result<(), Status>` are used identically in every task and in the loop wiring.
- **Visibility:** `pub(crate)` on the type/methods/handlers; struct fields stay private (mod.rs uses only `new`/`re_watch`); `WatchEventRaw` private; `PLUGIN_METADATA_KIND` reached via `super::` without a visibility change.
