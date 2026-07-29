# CCI-08: Unified ConfigData Write Model — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put every Center-initiated ConfigData write on one model — payload built from the
local watch-cache document, a CAS precondition, no pre-write fresh GET, no auto-retry on
conflict — and replace the current "sleep, then guess" convergence with an observed,
honestly-reported outcome.

**Architecture:** One shared write core in center-app owns the whole sequence: read the
cached document, capture its `resourceVersion` as the CAS precondition, apply a
caller-supplied mutation, PUT it back through the proxy tunnel, then watch the *local*
cache until the document's version leaves the precondition. Two independent signals decide
the outcome — version moved (the watch caught up) and a caller-supplied predicate (the
intent is currently in effect) — so "propagated" and "still true" are never conflated.
Failover, Selector switching, and row-level sync all become callers of that core; the
Controller's dedicated failover endpoints stop being used.

**Tech Stack:** Rust (axum, serde_json), React + TypeScript. Repository:
`/Volumes/ExtStore/ws5/EdgionCenter` (branch feature-0716, base c5d238c).

**Spec:** `docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md` §3 (write model) and §7 CCI-08, including the "Convergence wait — design decided 2026-07-27" subsection, which this plan implements literally.

## Global Constraints

- Everything written to disk must be English (Chinese only in `web/src/i18n/zh.ts` UI
  strings). Commits authorized (local only, NEVER push).
- **No pre-write fresh GET.** The write payload and the CAS precondition both come from the
  local watch cache. This is the decision that makes the convergence predicate sound (a CAS
  write pins the change to the precondition version, so a version that leaves it can only
  have advanced to at or past this write).
- **Every write carries a CAS precondition.** PUT sends `If-Match` *and* keeps
  `metadata.resourceVersion` in the body; DELETE sends `If-Match`. A write without a
  precondition is a bug — the core refuses to send it.
- **409 is terminal for the operation.** Report `conflict`; never auto-retry.
- **Convergence polls the LOCAL watch cache at 500 ms.** Never poll the Controller — that
  would rebuild the fan-out CCI-04 deleted. Budget is a flat 10 s; the loop returns the
  moment it observes convergence, so generosity is free.
- **Outcomes are honest and distinct**: `converged` (version moved + predicate true),
  `superseded` (version moved + predicate false — the write took effect and was then
  overwritten; return the observed current value), `accepted` (write succeeded but this
  replica cannot observe convergence), `conflict` (409), `failed` (write rejected),
  `unknown` (budget exhausted with the version unchanged). Never collapse `superseded` or
  `accepted` into a timeout or a bare success.
- **Non-owning replica returns `accepted` immediately** with that reason, instead of
  burning the budget. In Kubernetes the replica serving the request may not hold the
  Controller's federation session, and the watch cache is process-local.
- **Idempotent skip:** if the cached document already satisfies the predicate, skip the
  write entirely and report `converged`.
- Measured convergence latency is telemetry only — never an input to the timeout.
- Do NOT rewire `region_route_failover` (the `plugin_name` + `entry_index` form at
  ~line 704): it reads `list_region_routes()`, which is empty in production, and CCI-09
  deletes it. Leave it untouched.
- Do NOT delete the Controller's dedicated failover endpoints — CCI-09 owns that. This plan
  only stops calling them.

---

### Task 1: Runtime — the write-path lookup surface

**Files:**
- Modify: `crates/center-runtime/src/watch_cache/registry.rs` — add a non-creating per-controller document lookup
- Modify: `crates/center-runtime/src/proxy.rs` — publish the local-ownership check (~199)
- Test: both files' test modules

**Interfaces:**
- Produces (consumed by Task 2):

```rust
// registry.rs — on the WatchedConfigData specialization (place it next to list_all
// in read_model.rs if that reads better; keep it non-creating either way):
impl CenterWatchCacheRegistry<WatchedConfigData> {
    /// Raw, unredacted document for `key` ("namespace/name", or bare name for
    /// cluster-scoped) in `controller_id`'s cache, without creating a cache for
    /// an unknown controller. For the CCI-08 write path and its convergence
    /// checks only — never serialize the result into a global API response.
    pub fn raw_document(&self, controller_id: &str, key: &str) -> Option<Arc<WatchedConfigData>>;
}

// proxy.rs — visibility change plus a doc comment recording the dual meaning:
impl ProxyForwarder {
    /// Whether this replica holds a dispatchable local session for the
    /// Controller. It answers two questions at once: writes reach it without a
    /// replica hop, AND this process's watch cache is the one fed by that
    /// Controller — so only here can convergence be observed locally.
    pub fn local_session_is_dispatchable(&self, controller_id: &str) -> bool;
}
```

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn raw_document_does_not_create_a_cache_for_an_unknown_controller() {
        // registry with one seeded controller: raw_document("other/c", key) is None
        // AND the registry still reports exactly one cache afterwards (this is the
        // trap get_or_create would fall into).
    }

    #[test]
    fn raw_document_returns_the_unredacted_body() {
        // seed a Misc doc with a secret in /spec/data/config; raw_document returns
        // it intact (unlike list_entries, which redacts).
    }
```

Run `cargo test -p edgion-center-runtime --lib watch_cache`: both fail.

- [ ] **Step 2: Implement.** `raw_document` reads the caches map under its read lock, gets
the controller's cache if present, and delegates to the existing `raw_entry`. Change
`local_session_is_dispatchable` from `fn` to `pub fn` and add the doc comment above; do not
change its logic.

- [ ] **Step 3: Run** `cargo test -p edgion-center-runtime --lib` and `cargo clippy -p edgion-center-runtime --all-targets`.

- [ ] **Step 4: Commit** (`feat(watch_cache): non-creating raw document lookup for the write path` + co-author trailer)

---

### Task 2: The shared write core

**Files:**
- Create: `crates/center-app/src/api/config_data_ops.rs`
- Modify: `crates/center-app/src/api/mod.rs` — declare the module
- Test: the new file's test module

**Interfaces:**
- Consumes: Task 1's `raw_document` and `local_session_is_dispatchable`; `state.proxy.forward`; `ApiState`.
- Produces (consumed by Tasks 3-4):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutcomeState { Converged, Superseded, Accepted, Conflict, Failed, Unknown }

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutcome {
    pub controller_id: String,
    pub state: OutcomeState,
    /// Why the terminal state was reached — e.g. "not the owning replica",
    /// "overwritten after the write landed", the upstream error. Absent on a
    /// clean `converged`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The document as last observed in the local cache. Present for
    /// `superseded` so the operator sees what is actually in effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<serde_json::Value>,
    /// Measured time from write dispatch to observation. Telemetry only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence_ms: Option<u64>,
}

/// Read the cached document, apply `mutate`, write it back under a CAS
/// precondition, then observe convergence locally.
///
/// `predicate` answers "is the intent currently in effect?" against a document.
/// It is checked twice: before writing (idempotent skip) and during convergence.
pub async fn write_config_data(
    state: &ApiState,
    controller_id: &str,
    namespace: &str,
    name: &str,
    mutate: &dyn Fn(&mut serde_json::Value) -> Result<(), String>,
    predicate: &dyn Fn(&serde_json::Value) -> bool,
) -> WriteOutcome;

pub const CONVERGENCE_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const CONVERGENCE_BUDGET: Duration = Duration::from_secs(10);
```

Sequence (implement exactly):
1. `raw_document(controller_id, "{namespace}/{name}")` → absent ⇒ `Failed` with reason
   "not in the local watch cache".
2. Extract `metadata.resourceVersion` → absent ⇒ `Failed` with reason "document has no
   resourceVersion" (never write without a precondition).
3. If `predicate(&doc)` already holds ⇒ `Converged` with `convergence_ms: Some(0)` and no
   write (idempotent skip).
4. Clone, `mutate`, PUT `/api/v1/namespaced/edgionconfigdata/{ns}/{name}` through
   `state.proxy.forward` with `If-Match: <precondition>`, `content-type: application/json`,
   and the mutated document (which still carries `metadata.resourceVersion`) as the body.
5. Response 409 ⇒ `Conflict`. Other non-2xx ⇒ `Failed` with the upstream status/body.
6. 2xx and `!local_session_is_dispatchable(controller_id)` ⇒ `Accepted`, reason "this
   replica does not hold the Controller session; convergence cannot be observed here".
7. Otherwise poll every `CONVERGENCE_POLL_INTERVAL` until `CONVERGENCE_BUDGET`:
   - version differs from the precondition **and** `predicate` holds ⇒ `Converged`
   - version differs **and** predicate does not ⇒ `Superseded`, with `observed`
   - version unchanged at the deadline ⇒ `Unknown`

- [ ] **Step 1: Failing tests** (drive the core directly with a seeded registry and a
`state` whose proxy has no live session — see the CCI-03 `watch_test_state` helper for the
builder pattern; for the paths that need a 2xx write you may need a small seam — if
`ProxyForwarder` cannot be faked from center-app, restructure the core so the dispatch step
is a parameter and note it in the report):

```rust
    #[tokio::test] async fn idempotent_skip_reports_converged_without_writing() {}
    #[tokio::test] async fn missing_document_or_version_fails_before_dispatch() {}
    #[tokio::test] async fn conflict_is_terminal_and_never_retried() {}
    #[tokio::test] async fn non_owning_replica_returns_accepted_immediately() {}
    #[tokio::test] async fn version_moved_with_predicate_false_reports_superseded_with_observed() {}
```

- [ ] **Step 2: Implement** the sequence above.
- [ ] **Step 3: Run** `cargo test -p edgion-center-app --lib api::config_data_ops` and clippy.
- [ ] **Step 4: Commit** (`feat(api): shared ConfigData write core with observed convergence` + co-author trailer)

---

### Task 3: Failover onto the core

**Files:**
- Modify: `crates/center-app/src/api/region_route_handlers.rs` — `direct_override_failover` (~832), `fan_out_failover` (~974) and its blind sleep (~1028), `wait_for_override_projection` (~907), `region_route_propagation_wait` (~1033), the constants (~37-38)
- Test: same file

**Interfaces:**
- Consumes: Task 2's `write_config_data`.
- Produces: the failover response carries per-controller outcomes instead of counts.

Behavior:
- For each online controller, call `write_config_data` with:
  - `mutate`: find the region by `region_name` under `/spec/data/config/regions`, set
    `failoverTo`; error if the region is absent (surfaces as `Failed` for that controller).
  - `predicate`: the same region's `failoverTo` equals the requested value.
- Aggregate: HTTP 200 when every controller is `Converged`; 207 when outcomes are mixed;
  502 when every controller `Failed`. Body carries the `WriteOutcome` list under `data`
  alongside the existing `modified`/`failed` counts (keep them — the web reads them today).
- Delete `wait_for_override_projection`, `region_route_propagation_wait`, both blind-sleep
  call sites, and `REGION_ROUTE_PROPAGATION_WAIT_MIN` / `_LIMIT`. The Controller's
  dedicated failover endpoint is no longer called from here (the endpoint itself dies in
  CCI-09).

- [ ] **Step 1: Failing test** — `failover_reports_per_controller_outcomes`: two seeded
controllers, one whose cached document already has the target `failoverTo` (idempotent skip
⇒ `Converged`), one absent from the cache (⇒ `Failed`); assert the response carries both
outcomes and a 207.
- [ ] **Step 2: Implement**, deleting the listed functions and constants.
- [ ] **Step 3: Run** `cargo test -p edgion-center-app --lib api::region_route_handlers` — update any test pinning the old blind-sleep timing or the old `{modified, failed}`-only shape, rather than deleting it.
- [ ] **Step 4: Commit** (`refactor(region-route): failover writes through the shared core` + co-author trailer)

---

### Task 4: Selector switching and row-level sync onto the core

**Files:**
- Modify: `crates/center-app/src/api/global_connection_ip_restriction_handlers.rs` — the active-profile switch handlers (~265-490)
- Modify: `crates/center-app/src/api/region_route_handlers.rs` — `sync_watched_override` (~184) and `upsert_resource` / `read_resource` / `prepare_resource_document` if they lose their last caller
- Test: both files

Behavior:
- Selector switch: `mutate` sets `/spec/data/config/active`; `predicate` compares it.
- Row-level sync: the source document comes from the *source* controller's cache; `mutate`
  replaces the target's `/spec/data` with the source's; `predicate` compares `/spec/data`.
  The explicit source→target semantics, preview/confirm flow, and the ban on ambiguous
  identities are unchanged — only the write and wait change.
- Any of `upsert_resource` / `read_resource` / `prepare_resource_document` left with no
  caller is deleted; if one survives, say which caller keeps it alive.

- [ ] **Step 1: Failing tests** — one per handler asserting the outcome shape and the
idempotent skip.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** the center-app suite and clippy.
- [ ] **Step 4: Commit** (`refactor(api): Selector and row sync write through the shared core` + co-author trailer)

---

### Task 5: Web — render the outcomes

**Files:**
- Modify: `web/src/api/regionRoute.ts` and the Selector/GIR client as needed — types for `WriteOutcome` / `OutcomeState`
- Modify: the RegionRoute failover UI and the Selector switch UI — outcome rendering
- Modify: `web/src/i18n/en.ts` + `web/src/i18n/zh.ts` (line-parallel)
- Test: the affected page tests

Behavior: the operator must be able to tell the six outcomes apart. Minimum bar —
`converged` reads as success; `superseded` says the change took effect and was then
overwritten and shows the observed value; `accepted` says it was written but this replica
could not confirm; `conflict` says the view was stale and to refresh; `failed` and
`unknown` are distinct from both success and each other. Reuse existing tag vocabulary
where it fits rather than inventing a parallel one.

- [ ] **Step 1: Failing tests** for the outcome rendering.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** `npm run test -- --run` and `npm run build` in `web/`.
- [ ] **Step 4: Commit** (`feat(web): show ConfigData write outcomes` + co-author trailer)

---

### Task 6: Docs and repository verification

- [ ] **Step 1: Docs.** Update anything describing the old failover path, the blind sleep,
or "success means converged". Grep `docs/` and `skills/` for `wait_for_override_projection`,
`propagation`, and the failover endpoints; fix only what is now factually wrong.
- [ ] **Step 2: Verification** (cwd repo root, up to 600000 ms per cargo command):
  `cargo fmt --all` && `cargo check --workspace --all-targets` && `cargo clippy --workspace --all-targets` && `cargo test -p edgion-center-runtime -p edgion-center-app -p edgion-center-standalone -p edgion-center-kubernetes -p edgion-center-adapter-kubernetes -p edgion-center-adapter-sql`; then in `web/`: `npm run test -- --run` && `npm run build`.
  Known out-of-scope: the 11 pre-existing failures rooted in the `edgion-data`
  platform-namespace fixture (`global_resource_planner`, adapter `global_resource_store`,
  adapter `global_resources`). Trivial fmt/clippy fallout → `chore: appease fmt/clippy for cci-08`.
- [ ] **Step 3: Confirm the old machinery is gone**

Run: `grep -rn "wait_for_override_projection\|region_route_propagation_wait\|REGION_ROUTE_PROPAGATION_WAIT" crates web/src`
Expected: no hits.

- [ ] **Step 4: Report** files changed, checks, deviations. Do not push. Stage explicit
paths — the worktree has pre-existing untracked `docs/` and `tasks/` content that must stay
untracked.
