# CCI-02: ConfigData Watch Read Model — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the Center's federation ConfigData watch cache (batch atomicity,
version-gap detection, staleness, capacity) and turn it into the queryable all-types read
model with revision/type-change publication that CCI-03 (GlobalResources API) and CCI-06
(SSE) consume.

**Architecture:** `CenterWatchCache` stays the single store (spec: "no second cache").
Hardening happens at the two apply seams (`federation/server.rs` parse layer +
`watch_cache/cache.rs` apply layer). The cache gains per-controller `revision` / `stale` /
`overflowed` / `last_applied_unix_ms` state and a bounded broadcast of type-scoped change
summaries. A new `watch_cache/read_model.rs` adds production read accessors specialized to
`WatchedConfigData` (all six type strings, `Misc` config redacted in the global
projection). The `CenterMetaDataStore` override projection is unchanged except for the
handler-trait signature (removals now carry values, so type-scoped changes are computable).

**Tech Stack:** Rust, tokio (broadcast), parking_lot, serde_json. Single repository:
`/Volumes/ExtStore/ws5/EdgionCenter` (branch feature-0716, base commit 94ec174).

**Spec:** `docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md` §7 CCI-02 entry (and the CCI-03/CCI-06 entries it feeds).

## Global Constraints

- Everything written to disk must be English. Commits authorized (local only, NEVER push).
- Reuse `CenterWatchCache` — no second cache, no duplicate storage of ConfigData bodies.
- Batch atomicity: a malformed list or event batch must leave the previous snapshot
  completely unchanged and must NOT advance `sync_version`. Partial application is a bug.
- Exact values: per-controller entry cap `MAX_ENTRIES_PER_CONTROLLER = 10_000`; change
  broadcast capacity `256`; type strings are exactly `IpList`, `KeyList`, `Selector`,
  `Misc`, `RegionRouteOverride`, `ServiceRegionRouteOverride` (unknown/missing → `Unknown`).
- Redaction is fail-closed (adjudicated during review): `/spec/data/config` passes through
  the global read accessors ONLY for the five known-safe canonical types (`IpList`,
  `KeyList`, `Selector`, `RegionRouteOverride`, `ServiceRegionRouteOverride`); `Misc`,
  unknown, missing, or case-variant type strings are all redacted. The raw document stays
  in the cache for the CCI-08 write path (CAS needs the full body) — the redaction boundary
  is the read model, per the spec's "secrets never enter the global read model".
- Metric label discipline (fed_metrics.rs:9-13): never `controller_id` or free-form
  strings as label values; every new label value joins the drift test.
- Keep changes minimal; pre-release — signature changes are fine, shims are not. The
  `plugin_metadata`/`PLUGIN_METADATA_KIND` naming is NOT renamed in this plan (deferred).
  > Update 2026-07-28: `PLUGIN_METADATA_KIND` was since renamed to `EDGION_CONFIG_DATA` by
  > `tasks/new_work/18-watch-kind-constant-doubles-as-metric-label.md`, and the watch `kind`
  > metric label now comes from `FedWatchState.kind`. The `plugin_metadata` field name is
  > still deferred as written above.

---

### Task 1: Batch atomicity + version-gap detection + resync on parse errors

**Files:**
- Modify: `crates/center-runtime/src/federation/server.rs` — `apply_watch_list` (~748-807), `apply_watch_event` (~814-973), stream-loop watch arms (~1613-1682)
- Modify: `crates/center-runtime/src/observe/fed_metrics.rs` — `watch_error_reason` labels + drift test
- Test: `federation/server.rs` `mod tests` (helpers `make_pm_cache`/`pm_json`/`list_json`/`event_json` at ~2298-2350)

**Interfaces:**
- Produces: `apply_watch_list` rejects the WHOLE list on any keyless object or duplicate
  key (→ `ParseError`, cache untouched); `apply_watch_event` validates the WHOLE batch
  before applying (any malformed event, unknown event type, or keyless object → `ParseError`,
  cache untouched) and returns `ReWatch` on a non-monotonic batch version
  (`resp.sync_version <= cache.get_sync_version()` while the cache is non-empty-versioned);
  the stream loop routes `ParseError` into the existing 3 s backoff re-watch instead of
  silently ignoring it. New label `watch_error_reason::VERSION_GAP = "version_gap"`.

- [ ] **Step 1: Write the failing tests** (names fixed; bodies mirror the existing test style):

```rust
    #[test]
    fn apply_watch_list_rejects_batch_with_keyless_object() {
        // list_json with one valid doc + one doc missing metadata.name
        // => WatchOutcome::ParseError, cache sync_version stays 0, no entries.
    }

    #[test]
    fn apply_watch_list_rejects_duplicate_keys() {
        // two docs with the same ns/name => ParseError, cache untouched.
    }

    #[test]
    fn apply_watch_event_rejects_partial_batch() {
        // seed cache via a valid list (version 10); event batch with one valid
        // add + one keyless doc + version 11 => ParseError, cache still has
        // ONLY the seeded entries and sync_version 10.
    }

    #[test]
    fn apply_watch_event_unknown_type_rejects_batch() {
        // replaces the old apply_watch_event_unknown_type_skipped semantics:
        // an unknown event type now rejects the whole batch (ParseError),
        // valid sibling events are NOT applied. Delete the old test.
    }

    #[test]
    fn apply_watch_event_version_gap_rewatches() {
        // seed via list at version 20; event batch carrying sync_version 20
        // (non-monotonic) => WatchOutcome::ReWatch, cache untouched.
    }
```

Run: `cargo test -p edgion-center-runtime --lib federation::server` — the five fail (old
lenient behavior applies partial batches / silently skips).

- [ ] **Step 2: Implement**

- `apply_watch_list`: after deserializing, build keys via `resource_key`; if any object
  yields `None` OR a key repeats, `record_watch_list(kind, PARSE_ERROR)` + warn (include
  counts, not bodies) → return `WatchOutcome::ParseError` without touching the cache.
- `apply_watch_event`: first pass validates every raw event (parseable event type, body
  parses, key derivable) into a complete `Vec<WatchEventSimple>`; any failure →
  `record_watch_error(kind, PARSE_ERROR)` + warn → `ParseError`, cache untouched. Before
  applying, if `resp.sync_version <= pm_cache.get_sync_version()` and
  `pm_cache.get_sync_version() > 0` → `record_watch_error(kind, VERSION_GAP)` + warn →
  return `WatchOutcome::ReWatch`. An all-events-validated batch is then applied exactly as
  today (empty batches still skip the apply and do not advance the version).
- Stream loop: in BOTH the list and event arms, `WatchOutcome::ParseError` now takes the
  same backoff-re-watch block that `BackoffReWatch` uses (extract the block into a small
  closure/helper if duplication is ugly); the `_ => {}` arm then covers only `Skipped`.
- `fed_metrics.rs`: add `pub const VERSION_GAP: &str = "version_gap";` to
  `watch_error_reason` and to the drift test's expected set.

- [ ] **Step 3: Run tests** — the five new pass, all pre-existing watch tests pass except
the deliberately deleted `apply_watch_event_unknown_type_skipped`; `observe::fed_metrics`
suite passes.

- [ ] **Step 4: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add crates/center-runtime/src/federation/server.rs crates/center-runtime/src/observe/fed_metrics.rs
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "feat(federation): atomic watch batches with version-gap resync

A malformed list/event batch previously applied its valid subset and
still advanced the sync version; parse errors were silently swallowed
by the stream loop. Batches are now all-or-nothing, non-monotonic
versions trigger a re-watch, and parse errors backoff re-watch.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Cache state — revision, staleness, overflow capacity, change broadcast

**Files:**
- Modify: `crates/center-runtime/src/watch_cache/cache.rs` (CacheState, replace_all, apply_events, new state methods)
- Modify: `crates/center-runtime/src/watch_cache/registry.rs` (mark_offline sets stale; broadcast channel + subscribe)
- Modify: `crates/center-runtime/src/watch_cache/traits.rs` (`partial_update` removals carry values)
- Modify: `crates/center-runtime/src/watch_cache/mod.rs` (ConfigTyped impl for WatchedConfigData, re-exports)
- Modify: `crates/center-runtime/src/metadata_store.rs` (handler impl signature)
- Modify: `crates/center-runtime/src/federation/server.rs` (Overflow outcome arm; mark_offline already routes through registry)
- Modify: `crates/center-runtime/src/observe/fed_metrics.rs` (`watch_error_reason::OVERFLOW = "overflow"` + drift test)
- Test: cache.rs / registry.rs / metadata_store.rs test modules

**Interfaces:**
- Produces (consumed by Task 3, CCI-03, CCI-06):

```rust
// watch_cache/mod.rs
pub trait ConfigTyped {
    /// The ConfigData type discriminator, or None when absent/not a string.
    fn config_type(&self) -> Option<&str>;
}
impl ConfigTyped for serde_json::Value { /* reads /spec/data/type */ }

/// Type-scoped change summary for SSE/invalidations. Never carries bodies.
#[derive(Debug, Clone)]
pub struct ChangeSummary {
    pub controller_id: String,
    pub revision: u64,
    pub changed_types: std::collections::BTreeSet<String>, // "Unknown" for untyped docs
    pub event_unix_ms: u64,
}

pub const MAX_ENTRIES_PER_CONTROLLER: usize = 10_000;

// cache.rs — CacheState gains:
//   revision: u64 (monotonic per controller, +1 per APPLIED batch),
//   stale: bool, overflowed: bool, last_applied_unix_ms: Option<u64>
// CenterWatchCache gains:
pub struct CacheStatus {
    pub sync_version: u64,
    pub server_id: String,
    pub revision: u64,
    pub stale: bool,
    pub overflowed: bool,
    pub last_applied_unix_ms: Option<u64>,
    pub entry_count: usize,
}
impl<T: ConfigTyped> CenterWatchCache<T> {
    pub fn status(&self) -> CacheStatus;
    pub fn set_stale(&self);            // offline marking; cleared by next applied batch
}
// replace_all / apply_events return an ApplyResult:
pub enum ApplyResult { Applied, Overflow }
// Overflow: the batch would exceed MAX_ENTRIES_PER_CONTROLLER — nothing is
// applied, `overflowed` is set (cleared by the next batch that fits).

// traits.rs — signature change (values, not keys, for removals):
fn partial_update(&self, controller_id: &str,
                  add: HashMap<String, Arc<T>>,
                  update: HashMap<String, Arc<T>>,
                  remove: HashMap<String, Arc<T>>);

// registry.rs:
impl<T> CenterWatchCacheRegistry<T> {
    pub fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<ChangeSummary>;
    pub fn statuses(&self) -> Vec<(String, CacheStatus)>;
    // mark_offline additionally calls cache.set_stale() when the cache exists.
}
```

- [ ] **Step 1: Failing tests** (fixed names): `revision_increments_per_applied_batch`,
`stale_set_on_offline_cleared_on_next_batch`, `overflow_rejects_batch_and_flags`,
`change_summary_carries_changed_types_including_removals` (subscribe, apply a batch adding
a KeyList doc and removing a RegionRouteOverride doc, assert `changed_types ==
{"KeyList","RegionRouteOverride"}` and no bodies in the summary),
`metadata_store_handles_remove_values` (handler still removes override rows). Run —
all fail to compile/pass.

- [ ] **Step 2: Implement**

- Cache: state fields + `status()` + `set_stale`; `replace_all` computes changed types as
  the union of old-map and new-map types whose entry sets differ (wholesale replace may
  simply union all old + new types — document that coarseness; CCI-06 coalesces anyway);
  `apply_events` unions types of added/updated values and of the REMOVED values it looks
  up before deletion; both bump `revision`, set `last_applied_unix_ms`
  (`SystemTime::now()` → unix ms), clear `stale`/`overflowed` on success, and send a
  best-effort `ChangeSummary` through the broadcast sender (a lagging/full channel is
  dropped silently — `let _ =`). Capacity check happens before mutation: projected
  post-batch entry count > `MAX_ENTRIES_PER_CONTROLLER` → set `overflowed`, record
  `record_watch_error(kind, OVERFLOW)` at the server layer via the returned
  `ApplyResult::Overflow`, apply nothing.
- The broadcast `Sender<ChangeSummary>` lives in the registry (`broadcast::channel(256)`)
  and is passed to each cache at `get_or_create`.
- `server.rs`: `apply_watch_list`/`apply_watch_event` map `ApplyResult::Overflow` to a new
  `WatchOutcome::Overflow`; the stream loop treats it like `Terminal` (warn once with the
  bound name `MAX_ENTRIES_PER_CONTROLLER`, terminate the session's watch state, no
  re-watch storm). Add `OVERFLOW` to `watch_error_reason` + drift test.
- `metadata_store.rs`: adapt `partial_update` to the new removal signature (it only needs
  the keys; iterate `remove.keys()`).

- [ ] **Step 3: Run** `cargo test -p edgion-center-runtime --lib watch_cache metadata_store federation::server observe::fed_metrics` — all pass. `cargo check --workspace --all-targets` clean (test ApiState builders in center-app are unaffected — the trait change is internal to the handler impl).

- [ ] **Step 4: Commit** (message: `feat(watch_cache): revision, staleness, capacity, and type-scoped change broadcast` + co-author trailer)

---

### Task 3: Production read accessors (the CCI-03 surface)

**Files:**
- Create: `crates/center-runtime/src/watch_cache/read_model.rs`
- Modify: `crates/center-runtime/src/watch_cache/mod.rs` (module + re-exports)
- Test: read_model.rs test module

**Interfaces:**
- Produces (the API CCI-03 consumes; specialized to `WatchedConfigData`):

```rust
/// One ConfigData row in the global read model. `doc` is the stored document
/// EXCEPT for Misc, whose `/spec/data/config` is removed (redacted copy) —
/// the raw body never leaves the cache through this surface.
#[derive(Debug, Clone)]
pub struct ConfigDataEntry {
    pub controller_id: String,
    pub namespace: String,       // "" for cluster-scoped keys (bare name)
    pub name: String,
    pub config_type: String,     // one of the six type strings or "Unknown"
    pub doc: serde_json::Value,
}

impl CenterWatchCache<WatchedConfigData> {
    /// All entries, optional exact type filter, deterministic order
    /// (namespace, name). Misc docs are redacted.
    pub fn list_entries(&self, controller_id_hint: &str, type_filter: Option<&str>) -> Vec<ConfigDataEntry>;
    /// Raw (unredacted) document for the write path (CCI-08 CAS) and
    /// convergence checks. Doc-comment states it must never be serialized
    /// into a global API response.
    pub fn raw_entry(&self, key: &str) -> Option<Arc<WatchedConfigData>>;
}

impl CenterWatchCacheRegistry<WatchedConfigData> {
    /// Aggregate across controllers: (entries, per-controller CacheStatus).
    pub fn list_all(&self, type_filter: Option<&str>) -> Vec<ConfigDataEntry>;
}
```

- [ ] **Step 1: Failing tests** (fixed names): `list_entries_orders_and_filters_by_type`,
`misc_config_is_redacted_and_unrecoverable` (store a Misc doc with a secret-looking
config value; assert the listed doc has no `/spec/data/config` pointer and the secret
string does not appear anywhere in the serialized entry), `raw_entry_returns_full_doc`,
`list_all_carries_controller_identity`.

- [ ] **Step 2: Implement** — redaction clones the `Value` and removes the
`config` key from the `/spec/data` object (only for `config_type == "Misc"`); everything
else is `Arc`-cheap. Ordering: sort by `(namespace, name)`; registry aggregation sorts by
`(namespace, name, controller_id)`.

- [ ] **Step 3: Run** the watch_cache suite; then `cargo clippy -p edgion-center-runtime --all-targets` clean.

- [ ] **Step 4: Commit** (message: `feat(watch_cache): typed read accessors with Misc redaction` + co-author trailer)

---

### Task 4: Repository verification

- [ ] `cargo fmt --all` && `cargo check --workspace --all-targets` && `cargo clippy --workspace --all-targets` && `cargo test -p edgion-center-runtime -p edgion-center-app -p edgion-center-standalone -p edgion-center-kubernetes`
  (cwd `/Volumes/ExtStore/ws5/EdgionCenter`). Known out-of-scope: the 4 pre-existing
  `global_resource_planner` failures. Trivial fmt/clippy fallout → `chore: appease fmt/clippy for cci-02`.
- [ ] Report: files changed, checks, deviations. Do not push.
