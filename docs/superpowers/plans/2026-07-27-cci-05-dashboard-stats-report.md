# CCI-05: Dashboard Counts from StatsReport — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Center dashboard gets fleet resource counts from the Controller-pushed
`StatsReport` instead of listing every resource object through the proxy tunnel, and it
tells the operator apart three states that look identical today: counts missing, counts
frozen because the Controller is offline, and a genuine zero.

**Architecture:** `StatsReport.per_kind` already reaches the Center every 5 s and dies in
the aggregator behind `#[allow(dead_code)]`. This plan gives it a durable path
(observation → RuntimeProjector → `EdgionController` CRD status) so every replica can serve
it, exposes it plus a three-state freshness discriminator on `GET /api/v1/controllers`,
and rewrites the dashboard panel to sum the summaries it is already given — deleting the
per-kind-per-controller HTTP fan-out (20 requests per online Controller per refresh).

**Tech Stack:** Rust (serde, kube), React + TypeScript (vitest). Repository:
`/Volumes/ExtStore/ws5/EdgionCenter` (branch feature-0716, base c64113b).

**Spec:** `docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md` §7 CCI-05 entry, plus §2 F4 (the stats fallback the plan's earlier task left half-finished).

## Global Constraints

- Everything written to disk must be English (Chinese only in `web/src/i18n/zh.ts` UI
  strings). Commits authorized (local only, NEVER push).
- **Freshness is derived from liveness, never from a time threshold.** A Controller pushes
  only when its counts change, so "no report for a while" means *unchanged*, not stale. The
  three states are exactly:
  - `fresh` — counts present and the Controller is online,
  - `stale` — counts present and the Controller is offline (frozen at their last value),
  - `missing` — no counts at all (never reported, or reset by re-registration).
  Serialized as `stats_state` with values `"fresh" | "stale" | "missing"`.
- Per-kind counts are bounded on ingest: at most `MAX_STATS_KINDS = 64` entries, each key
  at most 63 characters. A report violating either bound keeps its `total` but stores **no**
  per-kind map (so the surface reports `missing` rather than a truncated lie) and logs one
  warning. Never truncate silently.
- Per-kind maps are `BTreeMap<String, u32>` end to end so CRD status serialization is
  deterministic and does not produce spurious writes.
- `ControllerSummary` keeps its existing **snake_case** wire shape (it is the one Center DTO
  that is not camelCase; changing it would break the dashboard). New fields follow suit:
  `per_kind`, `stats_state`.
- The Kubernetes CRD Rust struct (`crd.rs`) and the deployed schema
  (`cicd/deploy/center-kubernetes/crd.yaml`) must be changed together — a field absent from
  the structural schema is silently pruned by the API server.
- Existing stats lifecycle stays as it is: `mark_offline` preserves counts (that is what
  makes `stale` renderable), `upsert_registration` clears them, `remove` drops them.
- No new Prometheus metric in this plan (per-kind label cardinality is bounded but the
  spec does not ask for it; keep the diff focused).
- Do not touch the watch read model, GlobalResources, the planner, or the sync feature.

---

### Task 1: Durable per-kind plumbing (ingest → observation → CRD status)

**Files:**
- Modify: `crates/center-core/src/controller.rs` — `ControllerRecord` (~85-108), `ControllerRuntimeObservation` (~110-121)
- Modify: `crates/center-adapter-kubernetes/src/crd.rs` — `EdgionControllerStatus` (~48-93)
- Modify: `cicd/deploy/center-kubernetes/crd.yaml` — status schema (siblings at ~114-125)
- Modify: `crates/center-adapter-kubernetes/src/controller_directory.rs` — `project_runtime` merge (~476-481), `list()` mapping (~427-428), `mark_offline` copy-forward (~366-372), `upsert_registration` reset (~289-290)
- Modify: `crates/center-runtime/src/federation/server.rs` — `RuntimeProjectionSlot` (~77-80) + merge (~109-115), the `CtrlPayload::StatsReport` arm (~1820-1837)
- Modify: `crates/center-adapter-sql/src/lib.rs` — `list()` hardcoded `None`s (~460-465) gain the new field
- Test: `controller_directory.rs` tests, `server.rs` tests

**Interfaces:**
- Produces (consumed by Task 2):

```rust
// center-core/src/controller.rs — both structs gain, with camelCase serde on the record:
pub resource_counts_by_kind: Option<std::collections::BTreeMap<String, u32>>,

// center-runtime/src/federation/server.rs — ingest bounds:
const MAX_STATS_KINDS: usize = 64;
const MAX_STATS_KIND_LEN: usize = 63;
/// Returns None (and warns once per report) when the map violates a bound.
fn bounded_per_kind(per_kind: HashMap<String, u32>) -> Option<BTreeMap<String, u32>>;
```

CRD schema addition (structural, so the value type must be declared):

```yaml
resourceCountsByKind:
  type: object
  nullable: true
  additionalProperties:
    type: integer
    format: int32
    minimum: 0
```

- [ ] **Step 1: Write the failing tests**

In `crates/center-runtime/src/federation/server.rs` tests:

```rust
    #[test]
    fn bounded_per_kind_accepts_a_normal_report() {
        // 3 kinds -> Some(BTreeMap) with the same entries.
    }

    #[test]
    fn bounded_per_kind_rejects_oversized_maps_instead_of_truncating() {
        // MAX_STATS_KINDS + 1 entries -> None.
        // One entry whose key exceeds MAX_STATS_KIND_LEN -> None.
    }
```

In `crates/center-adapter-kubernetes/src/controller_directory.rs` tests, extend the
existing `runtime_projection_is_session_and_ownership_fenced` fixture (it already asserts
`resource_count == Some(42)`) with a per-kind map, and add:

```rust
    #[tokio::test]
    async fn offline_preserves_per_kind_counts() {
        // project a per-kind map, mark_offline, then list(): the map survives
        // alongside resource_count (this is what makes the "stale" state renderable).
    }
```

Run the two suites: all four fail (field does not exist).

- [ ] **Step 2: Implement the type + schema.** Add the field to `ControllerRecord`,
`ControllerRuntimeObservation`, `EdgionControllerStatus`, and the deployed CRD schema.
Keep the doc comments consistent with the neighbouring `resource_count` field.

- [ ] **Step 3: Implement ingest bounds and the observation.** Add `MAX_STATS_KINDS`,
`MAX_STATS_KIND_LEN`, and `bounded_per_kind`. In the `StatsReport` arm, clone what the
aggregator needs before moving (the aggregator still takes the `HashMap`), and set
`resource_counts_by_kind: bounded_per_kind(...)` on the submitted observation.

- [ ] **Step 4: Implement the projector merge and the adapter paths.** `RuntimeProjectionSlot`
copies the new field when `Some` (same shape as `resource_count`); the K8s adapter writes
it in `project_runtime`, reads it in `list()`, copies it forward in `mark_offline`, and
resets it to `None` in `upsert_registration`; the SQL adapter's `list()` hardcodes `None`
for it like its siblings.

- [ ] **Step 5: Run**

`cargo test -p edgion-center-runtime -p edgion-center-adapter-kubernetes -p edgion-center-adapter-sql --lib`
Expected: all pass (minus the 4 known `global_resource_planner` failures).

- [ ] **Step 6: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add -u
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "feat(controllers): persist per-kind resource counts from StatsReport

per_kind reached the Center every 5s and died in the in-process
aggregator, so a multi-replica Kubernetes deployment could only ever
serve a fraction of it. It now rides the runtime observation into the
EdgionController status like the scalar total. Oversized or malformed
maps are dropped rather than truncated.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Surface counts and freshness on the controllers API

**Files:**
- Modify: `crates/center-runtime/src/aggregator.rs` — `StatsEntry` (~21-31, drop the `#[allow(dead_code)]`), `ControllerSummary` (~238-255), `controller_summaries` (~214-229)
- Modify: `crates/center-app/src/api/mod.rs` — `ApiState::controller_summaries` (~185-232)
- Test: both files' test modules

**Interfaces:**
- Consumes: Task 1's `ControllerRecord.resource_counts_by_kind`.
- Produces (consumed by Task 3):

```rust
// aggregator.rs — snake_case wire shape, matching the rest of ControllerSummary:
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatsState { Fresh, Stale, Missing }

pub struct ControllerSummary {
    // ... existing fields unchanged ...
    pub per_kind: Option<std::collections::BTreeMap<String, u32>>,
    pub stats_state: StatsState,
}
```

Derivation, identical in both `controller_summaries` paths:

```
counts present && online  -> Fresh
counts present && !online -> Stale
no counts                 -> Missing
```
where "counts present" means `key_count.is_some()`.

Also in this task — finish the F4 fallback the earlier plan left half-done: in the
directory-composed path, `stats_updated_secs_ago` gains the same `or_else(aggregator)`
fallback `key_count` already has, so standalone-SQL deployments stop reporting `null`
forever. `per_kind` uses the same `record ... or_else(aggregator)` shape.

- [ ] **Step 1: Write the failing tests**

In `aggregator.rs` (there is currently no test for `update_stats` at all):

```rust
    #[test]
    fn summaries_expose_per_kind_and_freshness() {
        // set_controller_info + update_stats -> Fresh, per_kind present, key_count = total.
        // mark_offline -> Stale, counts retained.
        // a controller with no stats at all -> Missing, key_count None, per_kind None.
    }
```

In `crates/center-app/src/api/mod.rs` tests (there is currently no test for the
no-directory path, and none for `stats_updated_secs_ago` anywhere):

```rust
    #[tokio::test]
    async fn summaries_fall_back_to_the_aggregator_when_the_directory_lacks_stats() {
        // Directory record with resource_count: None and stats_updated_unix_ms: None
        // (the SQL-adapter shape), aggregator holding stats for the same controller:
        // key_count, per_kind, and stats_updated_secs_ago all come from the aggregator,
        // and stats_state is Fresh for an online record.
    }

    #[tokio::test]
    async fn summaries_report_missing_and_stale_states() {
        // No stats anywhere -> Missing. Offline record with counts -> Stale.
    }
```

Run both suites: fail (fields/variants do not exist).

- [ ] **Step 2: Implement.** Expose `per_kind` from the aggregator (remove the
`#[allow(dead_code)]` and its now-false comment), add `StatsState`, compute it in both
paths, and add the two `or_else` fallbacks. Keep `last_seen_secs_ago` behaviour exactly as
it is.

- [ ] **Step 3: Run** `cargo test -p edgion-center-runtime -p edgion-center-app --lib`
and `cargo clippy -p edgion-center-app -p edgion-center-runtime --all-targets`.

- [ ] **Step 4: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add -u
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "feat(api): expose per-kind counts and a stats freshness state

stats_state distinguishes counts that are live, counts frozen by an
offline Controller, and counts that were never reported - three cases
the dashboard could not tell apart. Freshness is derived from liveness,
not a timeout: a Controller only pushes when its counts change.
stats_updated_secs_ago gains the aggregator fallback key_count already
had, so SQL-directory deployments stop reporting null.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Dashboard reads the summaries; delete the fan-out

**Files:**
- Delete: `web/src/hooks/useControllerResourceSnapshots.ts` and `web/src/hooks/useControllerResourceSnapshots.test.ts`
- Modify: `web/src/pages/Center/ResourceOverviewPanel.tsx` — rewrite off `ControllerSummary[]`
- Modify: `web/src/pages/Center/ResourceOverviewPanel.test.tsx` — drop the hook mock, drive it with summaries
- Modify: `web/src/pages/Center/ControllersPage.tsx` — the Resources column (~185-189)
- Modify: `web/src/pages/Center/ControllersPage.test.tsx` — add Resources-column assertions
- Modify: `web/src/api/center.ts` — `ControllerSummary` type (~16-29); delete `listControllerResources` (~58-69) **only if** no other caller survives (grep first)
- Modify: `web/src/i18n/en.ts` + `web/src/i18n/zh.ts` — keep the files line-parallel
- Check: `web/src/utils/controller-observability.ts` — if `ControllerResourceSnapshot`, `observations()`, or `buildConsistencyRows()` have no surviving caller after the hook dies, delete them and their tests; if they do, leave them alone and say which caller keeps them alive

**Interfaces:**
- Consumes: Task 2's wire fields.
- Produces:

```ts
export type StatsState = 'fresh' | 'stale' | 'missing'
export interface ControllerSummary {
  // ... existing ...
  per_kind?: Record<string, number> | null
  stats_state: StatsState
}
```

Panel behaviour (no network calls at all — it renders from the `controllers` prop it
already receives):
- Sum `per_kind` across **online** controllers, per catalog kind, preserving the existing
  card order from `listFirstClassResources()`.
- A controller that is online but has `stats_state === 'missing'` makes the totals
  incomplete: render the existing `+` suffix convention (`` `${n}+` ``) and the existing
  partial-data wording, exactly as the current panel does for unavailable kinds.
- Kinds absent from every `per_kind` map render `0` (a Controller that reports stats but
  omits a kind genuinely has none of it).

ControllersPage Resources column:
- `missing` → `—`
- `fresh` → the number
- `stale` → the number plus the existing stale tag vocabulary
  (`globalResources.syncState.stale` already exists in both i18n files with an orange tag;
  reuse that wording rather than inventing a second one, or add a `center.stats.stale` key
  if reuse reads wrong in context — decide and note which)

- [ ] **Step 1: Grep for surviving callers first**

Run: `grep -rn "useControllerResourceSnapshots\|listControllerResources\|ControllerResourceSnapshot\|buildConsistencyRows\|observations(" web/src`
Record what survives; that determines how much of `controller-observability.ts` dies.

- [ ] **Step 2: Write the failing tests.** Rewrite `ResourceOverviewPanel.test.tsx` to pass
summaries with `per_kind` and assert the summed per-kind cards and the partial-data case;
add a `ControllersPage.test.tsx` case asserting `—` / number / stale rendering for the
three states.

- [ ] **Step 3: Implement** the panel rewrite, the column, the types, and the i18n keys;
delete the hook, its test, and whatever Step 1 proved dead.

- [ ] **Step 4: Run** `npm run test -- --run` and `npm run build` in `web/`.

- [ ] **Step 5: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add -A -- web
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "feat(web): dashboard counts come from StatsReport

The overview panel listed every resource object of 19 kinds through the
proxy tunnel on every refresh - 20 requests per online Controller - to
compute numbers the Controller already pushes. It now sums the counts
already present on the controller summaries, and the Resources column
distinguishes live counts, counts frozen by an offline Controller, and
counts that were never reported.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Docs and repository verification

- [ ] **Step 1: Docs.** Update whatever now describes the deleted fan-out or claims
per-kind stats are unused — check `crates/center-runtime/src/aggregator.rs` doc comments
(the "not yet exposed" comment must go with the code), and grep `docs/` and `skills/` for
`useControllerResourceSnapshots`, "resource overview", and StatsReport descriptions; fix
only what is now factually wrong.
- [ ] **Step 2: Verification** (cwd repo root, up to 600000 ms per cargo command):
  `cargo fmt --all` && `cargo check --workspace --all-targets` && `cargo clippy --workspace --all-targets` && `cargo test -p edgion-center-runtime -p edgion-center-app -p edgion-center-standalone -p edgion-center-kubernetes -p edgion-center-adapter-kubernetes -p edgion-center-adapter-sql`; then in `web/`: `npm run test -- --run` && `npm run build`.
  Known out-of-scope: the 4 pre-existing `global_resource_planner` failures. Trivial
  fmt/clippy fallout → `chore: appease fmt/clippy for cci-05`.
- [ ] **Step 3: Confirm the fan-out is gone**

Run: `grep -rn "useControllerResourceSnapshots\|listControllerResources" web/src`
Expected: no hits (or only what Step 1 of Task 3 proved must survive, with the reason).

- [ ] **Step 4: Report** files changed, checks, deviations. Do not push. Stage explicit
paths — the worktree has pre-existing untracked `docs/` and `tasks/` content that must
stay untracked.
