# CCI-03: GlobalResources on the Watch Read Model — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve the GlobalResources EdgionConfigData list/detail from the in-memory
federation watch read model (zero Controller HTTP on the read path), with per-member
controller identity, sync state, freshness, and revision on the wire and in the UI.

**Architecture:** The center-app list/detail handlers branch on kind: `edgion-config-data`
takes a new watch-backed path reading `state.sync_client.plugin_metadata`
(`list_all` + `statuses`); the four other kinds keep the existing fan-out untouched (CCI-04
deletes them wholesale). The wire shape is extended additively (new optional camelCase
fields), so the fan-out kinds' responses are byte-identical. The existing `gr1.`
continue-token machinery is reused with watch-derived membership/inventory revisions, so
stale-token semantics carry over. One runtime gap is closed first: a watch that terminates
(Forbidden) or overflows now marks its cache stale so freshness is honest.

**Tech Stack:** Rust (axum, sha2, serde_json), React + TypeScript (vitest). Repository:
`/Volumes/ExtStore/ws5/EdgionCenter` (branch feature-0716, base 80a168b).

**Spec:** `docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md` §7 CCI-03 entry.

## Global Constraints

- Everything written to disk must be English. Commits authorized (local only, NEVER push).
- The watch-backed read path makes ZERO Controller HTTP calls — no `ControllerHttpClient`,
  no `GlobalResourcesService`, no proxy use anywhere in the new branch.
- Misc bodies stay redacted: the new path consumes ONLY `list_entries`/`list_all` (never
  `raw_entry`), and a response-level test proves a Misc secret cannot appear.
- No platform-namespace filtering on the watch path: every namespace in the cache is
  served; namespace is row identity only. (`platformNamespaces` stays in the catalog
  response for the four legacy kinds until CCI-04.)
- Wire compatibility: all new response fields are `Option` + `skip_serializing_if` so the
  four fan-out kinds' JSON is unchanged. Exact new field names: member-level `syncState`
  (`"ok" | "stale" | "overflowed"`), `freshnessUnixMs` (u64), `revision` (u64);
  cluster-level the same three.
- Cluster identity derives from the controller id's `{cluster}/{name}` invariant
  (`split_once('/')`; no slash → the whole id is the cluster) — the SPIFFE binding enforces
  this format at registration.
- Fan-out internals, preflight, and the four legacy kinds must NOT be touched (CCI-04's
  deletion inventory). The planner (`global_resource_planner.rs`) must not be touched.
- Frontend: en.ts/zh.ts stay line-parallel; new keys added to both.
- Keep changes minimal; pre-release, no shims.

---

### Task 1: Runtime — terminated/overflowed watches mark the cache stale

**Files:**
- Modify: `crates/center-runtime/src/federation/server.rs` — the stream loop's `Terminal` and `Overflow` arms
- Test: same file `mod tests`

**Interfaces:**
- Consumes: `CenterWatchCache::set_stale()` (exists), `status().stale`.
- Produces: after a `Terminal` (Forbidden) or `Overflow` outcome, `pm_cache.status().stale == true`, so CCI-03's freshness fields are honest for denied/overflowed controllers.

- [ ] **Step 1: Failing tests** — in the server tests, mirror `apply_watch_event_forbidden_is_terminal`'s setup but drive the OUTCOME handling contract: since the arms live in the stream loop (untestable directly), extract the two arm bodies' cache-marking into the apply layer instead — the cleanest seam: in `apply_watch_event`, the `Forbidden` branch already calls `pm_watch.terminate()`; add `pm_cache.set_stale()` there; in both apply fns' `Overflow` mapping, the cache already sets `overflowed` internally (T2 of CCI-02) — verify `status().overflowed` implies the UI-stale contract by ALSO setting `stale` inside the cache's overflow path (`cache.rs`). Tests:

```rust
    #[test]
    fn forbidden_terminal_marks_cache_stale() {
        // Forbidden event => Terminal, and pm_cache.status().stale == true.
    }

    #[test]
    fn overflow_marks_cache_stale() {
        // (in cache.rs tests) an ApplyResult::Overflow leaves stale == true
        // as well as overflowed == true; a later fitting batch clears both.
    }
```

- [ ] **Step 2: Implement** — one line in the `Forbidden` branch of `apply_watch_event` (`pm_cache.set_stale()` next to `pm_watch.terminate()`); in `cache.rs`, the overflow early-return additionally sets `state.stale = true` (both `replace_all` and `apply_events` overflow paths; the existing success path already clears both flags).

- [ ] **Step 3: Run** `cargo test -p edgion-center-runtime --lib federation::server watch_cache` — all pass.

- [ ] **Step 4: Commit** (`fix(federation): mark watch cache stale on terminal and overflow` + co-author trailer)

---

### Task 2: center-app — watch-backed EdgionConfigData list/detail

**Files:**
- Modify: `crates/center-app/src/api/global_resources.rs` — list/detail handlers branch + DTO extension + new watch module section
- Test: same file `mod tests` + router-level tests in `api/mod.rs` if needed

**Interfaces:**
- Consumes: `state.sync_client.plugin_metadata` (`CenterWatchCacheRegistry<WatchedConfigData>`): `list_all(type_filter)`, `statuses()`; `ConfigDataEntry { controller_id, namespace, name, config_type, doc }`; `CacheStatus { revision, stale, overflowed, last_applied_unix_ms, entry_count, .. }`. Existing app-side machinery reused verbatim: `ConfigDataFilter`/`parse_config_data_filter`, `parse_list_query`, `normalized_clusters`, `cluster_query_revision`, `ContinueToken`/`encode_token`/`decode_token`, `GroupKey`/`ComparisonGroup`/`ComparisonMember`/`ClusterResult`/`ListResponse`/`DetailResponse`, `catalog_revision`, error helpers.
- Produces:

```rust
// DTO extensions (all Option + skip_serializing_if = "Option::is_none"):
struct ComparisonMember { cluster, controller_id, object,
    sync_state: Option<WatchSyncState>, freshness_unix_ms: Option<u64>, revision: Option<u64> }
struct ClusterResult { cluster, state, controller_id, candidates, complete, errors,
    sync_state: Option<WatchSyncState>, freshness_unix_ms: Option<u64>, revision: Option<u64> }

#[derive(Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
enum WatchSyncState { Ok, Stale, Overflowed }

// New functions (same file):
fn watch_sync_state(status: &CacheStatus) -> WatchSyncState; // overflowed > stale > ok
fn watch_cluster_of(controller_id: &str) -> &str;            // split_once('/')
fn watch_membership_revision(statuses: &[(String, CacheStatus)]) -> String; // sha256 over sorted (id, revision, stale, overflowed)
fn watch_inventory_revision(clusters: &[ClusterResult], groups: &[ComparisonGroup]) -> String; // sha256 over canonical JSON
async fn list_config_data_from_watch(state, filter, clusters_param, limit, continue_token) -> Response;
async fn detail_config_data_from_watch(state, namespace, name, cluster) -> Response;
```

Behavior contract:
- `list` handler: when the parsed kind is `edgion-config-data`, delegate to
  `list_config_data_from_watch`; other kinds fall through to the existing fan-out code
  untouched.
- Rows: group `list_all(filter)` entries by `(namespace, name)`; each member carries its
  controller's `sync_state`/`freshness_unix_ms` (= `last_applied_unix_ms`)/`revision` from
  `statuses()`. `object` is the (already Misc-redacted) `doc`.
- `clusters` array: one `ClusterResult` per controller in `statuses()` (state `available`
  when not stale, `offline` when stale; `complete = !stale && !overflowed`; empty errors;
  `candidates` empty), filtered by the `cluster` query params when present (unknown
  requested cluster → existing `cluster_not_found` error). Cluster filtering also filters
  group members (drop groups with zero remaining members).
- Pagination: sort groups by `GroupKey` (existing ordering), slice by limit, reuse
  `ContinueToken` with `membership_revision = watch_membership_revision(..)` and
  `inventory_revision = watch_inventory_revision(..)`; a token whose revisions no longer
  match → existing `stale_continue_token` error. Deterministic order guaranteed by
  read-model sort + a final stable sort on `(namespace, name)` group keys.
- `detail` handler: for `edgion-config-data`, find the entry for `(namespace, name)` whose
  controller maps to the required `cluster` param via `list_all(None)` — no HTTP; response
  reuses `DetailResponse` with the redacted `object`, `complete = !stale`, empty errors,
  plus the member-level state fields are NOT part of DetailResponse (unchanged shape).
  Unknown cluster → `cluster_not_found`; no matching entry → `global_resource_not_found`.
- The watch path never touches `state.global_resources` — it works even when that service
  is `None`... BUT route mounting is gated on `capabilities.global_resources_inventory &&
  state.global_resources.is_some()` (mod.rs:406). Leave the gate as-is this task (CCI-04
  re-gates when the service dies); note it in the report.

- [ ] **Step 1: Failing tests** (same file's `mod tests`; seed the watch registry through
`state.sync_client.plugin_metadata.get_or_create("cluster-a/c0")` + `replace_all` with
`pm_json`-style docs — the `state_with_authz_mode` builder already constructs the
registry):

```rust
    #[tokio::test] async fn watch_list_serves_config_data_without_controller_http() { /* seed 2 controllers, GET list, assert groups/members/syncState/freshness/revision fields and that state.global_resources stayed None */ }
    #[tokio::test] async fn watch_list_type_filter_and_misc_redaction() { /* seed IpList + Misc-with-secret; filter=Misc; assert secret absent from the whole response body; filter=IpList returns config */ }
    #[tokio::test] async fn watch_list_pagination_and_stale_token() { /* limit=1 -> continueToken; second page ok; mutate cache (replace_all) -> old token => stale_continue_token */ }
    #[tokio::test] async fn watch_list_cluster_filter_and_stale_state() { /* mark one controller offline via registry.mark_offline -> its cluster state == "offline", members syncState == "stale"; cluster= filter narrows; unknown cluster -> cluster_not_found */ }
    #[tokio::test] async fn watch_detail_finds_member_without_http() { /* detail by ns/name/cluster; not-found and unknown-cluster errors */ }
```

- [ ] **Step 2: Implement** per the contract above. Keep the new code in a clearly-marked
`// ---- watch-backed EdgionConfigData path (CCI-03) ----` section of the same file.

- [ ] **Step 3: Run** `cargo test -p edgion-center-app --lib api::global_resources` and the full `api` module; `cargo clippy -p edgion-center-app --all-targets` clean. Existing fan-out tests must pass unchanged (wire-compat proof).

- [ ] **Step 4: Commit** (`feat(api): serve GlobalResources EdgionConfigData from the watch read model` + co-author trailer)

---

### Task 3: Frontend — per-member sync state, freshness, revision

**Files:**
- Modify: `web/src/api/globalResources.ts` (types only, additive)
- Modify: `web/src/pages/GlobalResources/GlobalResourceComparisonTable.tsx` (freshness/state column)
- Modify: `web/src/pages/GlobalResources/GlobalResourceComparisonDrawer.tsx` (per-member state line)
- Modify: `web/src/i18n/en.ts` + `web/src/i18n/zh.ts` (line-parallel keys)
- Test: `web/src/pages/GlobalResources/GlobalResourceInventoryPage.test.tsx`, `GlobalResourceComparisonTable.test.tsx`, `web/src/api/globalResources.test.ts`

**Interfaces:**
- Consumes: the Task 2 wire fields.
- Produces:

```ts
export type WatchSyncState = 'ok' | 'stale' | 'overflowed';
export interface GlobalResourceComparisonMember {
  cluster: string; controllerId: string | null; object: JsonObject;
  syncState?: WatchSyncState; freshnessUnixMs?: number; revision?: number;
}
// GlobalResourceClusterResult gains the same three optional fields.
```

Behavior:
- Table: new "Sync" column after Clusters — group-level worst state across members
  (`overflowed` > `stale` > `ok`; members without the field count as `ok`), rendered as a
  tag: `globalResources.syncState.ok` (green) / `.stale` (orange) / `.overflowed` (red).
- Drawer: under the member selector, one line per selected member:
  `t('globalResources.member.freshness')` + relative time from `freshnessUnixMs`
  (reuse the dashboard's existing relative-time helper if one exists; otherwise a local
  `formatRelativeMs` in the drawer file) + the sync-state tag.
- i18n keys added to BOTH en.ts and zh.ts at the same lines:
  `globalResources.column.sync`, `globalResources.syncState.ok`, `.stale`, `.overflowed`,
  `globalResources.member.freshness`.
- Tests: extend the inventory-page fixture members with the three fields and assert the
  tag renders; table test gets a stale-member case asserting the worst-state logic; the
  api test's `GlobalResourceListResponse` fixture gains the optional fields (compile-level
  check only). Do NOT touch the five-kind unions, preflight, or descriptors (CCI-04).

- [ ] **Step 1: Failing tests first** (extend the three test files as above; run `npm run test -- --run` in `web/`, expect the new assertions to fail).
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** `npm run test -- --run` (all pass) and `npm run build` (or the repo's typecheck script) clean.
- [ ] **Step 4: Commit** (`feat(web): surface watch sync state and freshness on GlobalResources` + co-author trailer)

---

### Task 4: Repository verification

- [ ] `cargo fmt --all` && `cargo check --workspace --all-targets` && `cargo clippy --workspace --all-targets` && `cargo test -p edgion-center-runtime -p edgion-center-app -p edgion-center-standalone -p edgion-center-kubernetes` (cwd repo root; known out-of-scope: the 4 pre-existing `global_resource_planner` failures).
- [ ] `cd web && npm run test -- --run` and the typecheck/build script.
- [ ] Trivial fmt/clippy fallout → `chore: appease fmt/clippy for cci-03`.
- [ ] Report: files changed, checks, deviations. Do not push.
