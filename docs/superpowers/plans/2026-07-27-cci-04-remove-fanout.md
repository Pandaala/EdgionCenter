# CCI-04: Remove the GlobalResources Fan-out — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the request-time Controller HTTP fan-out behind GlobalResources — the
five-kind inventory, the 2 s cache, the budget, single-flight, and preflight — leaving the
watch-backed EdgionConfigData path (CCI-03) as the only implementation.

**Architecture:** Deletion in dependency order. Preflight dies first (self-contained
chain). The `catalog` handler is then rewritten onto the watch registry so nothing but
dead code still needs `GlobalResourcesService`. The service, its submodules, the fan-out
branches, and the `ApiState` field then die in one atomic commit that also re-gates route
mounting (the watch path needs no service) and shrinks `API_KINDS` to EdgionConfigData.
The web client sheds the five-kind unions and preflight last.

**Tech Stack:** Rust (axum, serde), React + TypeScript (vitest). Repository:
`/Volumes/ExtStore/ws5/EdgionCenter` (branch feature-0716, base debcad3).

**Spec:** `docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md` §7 CCI-04 entry.

## Global Constraints

- Everything written to disk must be English. Commits authorized (local only, NEVER push).
- **`crates/center-core/src/global_resources.rs` is NOT modified.** Its
  `GLOBAL_RESOURCE_KINDS` (all five entries) and `GlobalResourceInventoryKind` (all
  variants) are the durable contract of the GlobalResource **sync** feature: the planner
  looks up `resource_kind` in that table with `.expect(...)` on the apply path
  (`global_resource_planner.rs:411`), and both stores deserialize the enum from persisted
  rows. Shrinking either would panic or fail to load existing sync resources. CCI-04
  narrows only the read-only inventory surface — `API_KINDS` in center-app.
- These must survive untouched and keep compiling: `global_resource_planner.rs`, the
  `/api/v1/center/global-resource-sync/*` surface, the generic proxy route +
  `ProxyForwarder`, `GlobalResourcesConfig` in both bins' config (the planner and both
  stores need `platform_namespaces`), and every watch-path symbol added by CCI-03.
- The six planner-shared symbols stay exported from `crates/center-runtime/src/global_resources.rs`:
  `request_target`, `resolve_authoritative_targets`, `stable_revision`,
  `ClusterResolutionState`, `ResolvedTarget`, `TargetResolutionMode` (plus their
  transitive needs `ClusterResolution` and `DEFAULT_MAX_TARGET_CLUSTERS`). The module keeps
  its current path and name — renaming it to `controller_targets` is explicitly deferred so
  the planner's imports stay untouched.
- The `errors` wire slot stays: `ClusterResult.errors` / `DetailResponse.errors` remain
  `Vec<PublicInventoryError>` (the watch path emits `[]`), because the dashboard drawer
  renders them and has tests. `PublicInventoryError` and `InventoryErrorCode` move into
  center-app as wire-only DTOs when their center-runtime home dies.
- `catalog_revision()` changes value when `API_KINDS` shrinks; every outstanding `gr1.`
  continuation token then returns `stale_continue_token`. That is intended.
- No behavior change to the watch path; its five CCI-03 tests must pass unmodified except
  where an `ApiState` literal field is removed.
- Keep changes minimal; pre-release — delete, never deprecate.

---

### Task 1: Delete preflight end to end

**Files:**
- Modify: `crates/center-app/src/api/global_resources.rs` — handler (~1018-1038), `PreflightResponse` (~1011-1016)
- Modify: `crates/center-app/src/api/mod.rs` — route (~420-423), doc header line 16
- Modify: `crates/center-runtime/src/global_resources.rs` — `preflight`/`preflight_cluster`/`preflight_cluster_with_deadline` (~450-509) and the whole preflight block (~1173-1497: `AccessEnvelope`, `ControllerAccessSnapshot`, `ResourceAccess`, `NamespacePreflightState`, `NamespacePreflight`, `KindPreflight`, `ControllerGlobalResourcesPreflight`, `PreflightError`, `preflight_controller`, `preflight_resolved_controller`, `preflight_impl`, `preflight_request`, `evaluate_kind_access`, `invalid_access_contract`, `is_sha256_revision`) and its tests (`write_exposure_requires_both_create_and_update`, `malformed_access_contract_fails_closed`, `preflight_is_read_only_and_probes_each_namespace_kind`, `false_success_and_invalid_revision_fail_before_namespace_probes`, `preflight_rejects_an_oversized_access_snapshot_before_parsing`)
- Modify: `crates/center-app/src/common/authz/catalog.rs` — `GLOBAL_RESOURCES_DIAGNOSE` const (~25), `all_keys()` entry (~106), the `PermissionGroup` key vec (~178), the route→permission arm (~303-305), the preflight block of `global_resource_routes_use_narrow_read_and_diagnose_permissions` (~1283-1303), the `preflight-v2` case in `global_resource_routes_require_exact_segment_boundaries` (~1315)
- Modify: `web/src/api/globalResources.ts` — `preflight` fn (~240-248) and preflight types (~56-60, ~145-174)
- Modify: `web/src/api/globalResources.test.ts` — the preflight assertions inside `uses explicit Center paths and always skips the Controller proxy` (~27, 39-48)
- Modify: `web/src/pages/GlobalResources/GlobalResourceInventoryPage.test.tsx` — `preflight: vi.fn()` mock (~26)
- Modify: `cicd/deploy/center-kubernetes/access-example.yaml` — the `global-resources:diagnose` lines (~45-46)

**Interfaces:**
- Produces: no `global-resources:diagnose` permission, no `/preflight` route, no preflight
  types in either language. `GLOBAL_RESOURCES_READ` and everything else in the authz
  catalog are unchanged.

- [ ] **Step 1: Delete the backend chain.** Remove the handler, route, doc line, service
methods, and the whole preflight block plus its five tests. The `FakeHttpClient` test
fixture (~1509-1540) and helpers `response` (~1542) / `full_access_body` (~1549) also lose
their last users — delete them only if `cargo test` proves no surviving test needs them
(the inventory says only preflight/inventory tests use them; inventory tests die in Task 3,
so if these fixtures are still needed by an inventory test, keep them until Task 3 and note
it in the report).

- [ ] **Step 2: Delete the permission.** Remove the const, its `all_keys()` entry, and its
slot in the `PermissionGroup` vec (leaving a single-key vec) and the route arm. Update the
two authz tests: drop the preflight expectations, keep the `global-resources:read` ones.
These edits must land together — a `PermissionGroup` naming a deleted const will not
compile.

- [ ] **Step 3: Delete the web half.** Remove the client fn, the four preflight interfaces,
the test assertions, and the page-test mock entry.

- [ ] **Step 4: Verify no residue**

Run: `grep -rn "preflight\|GLOBAL_RESOURCES_DIAGNOSE\|global-resources:diagnose" crates bins web/src cicd/deploy --include=*.rs --include=*.ts --include=*.tsx --include=*.yaml | grep -v target`
Expected: no hits outside `docs/`, `tasks/`, and `skills/` (docs are Task 5's concern).

- [ ] **Step 5: Test**

Run: `cargo test -p edgion-center-app -p edgion-center-runtime --lib` and, in `web/`,
`npm run test -- --run`.
Expected: all pass (minus the 4 known `global_resource_planner` failures).

- [ ] **Step 6: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add -u
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "refactor(global-resources): delete the preflight diagnostics surface

Preflight probed each platform namespace on every call through the
Controller proxy. The watch read model makes it unnecessary; its
permission, routes, DTOs, and client are removed.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Rewrite `catalog` onto the watch registry

**Files:**
- Modify: `crates/center-app/src/api/global_resources.rs` — `catalog` handler (~148-178), `CatalogResponse` (~115-123)
- Modify: `web/src/api/globalResources.ts` — `GlobalResourceCatalogResponse` (~81-87)
- Modify: `web/src/pages/GlobalResources/GlobalResourceInventoryPage.test.tsx` — catalog fixture (~31-40)
- Test: `crates/center-app/src/api/global_resources.rs` `mod tests`

**Interfaces:**
- Consumes: `state.sync_client.plugin_metadata.statuses()`, plus the CCI-03 helpers
  `watch_cluster_of` and `WATCH_CONFIG_REVISION`.
- Produces: `catalog` no longer reads `state.global_resources` (Task 3 can delete the
  field). `CatalogResponse` loses `platform_namespaces`; `config_revision` is
  `WATCH_CONFIG_REVISION`; `clusters` is derived from the watch registry exactly as the
  list path derives `ClusterResult` (one entry per controller: `cluster` from
  `watch_cluster_of`, `state` `Available` when not stale else `Offline`, `controller_id`
  set, `candidates` empty), sorted by `(cluster, controller_id)`.

- [ ] **Step 1: Write the failing test**

```rust
    #[tokio::test]
    async fn catalog_is_served_from_the_watch_registry() {
        // Seed two controllers via the CCI-03 watch_test_state helper: one healthy,
        // one marked offline through registry.mark_offline.
        // Call the catalog handler directly (like call_list/call_detail do).
        // Assert: clusters == the two derived entries with available/offline states,
        // configRevision == WATCH_CONFIG_REVISION, the response has no
        // "platformNamespaces" key, and kinds still lists the API_KINDS entries.
    }
```

Fill the body concretely by mirroring `watch_list_cluster_filter_and_stale_state`'s setup
and the `call_list` helper's dispatch style.

Run it: FAILS (catalog still calls the service, and `platformNamespaces` is present).

- [ ] **Step 2: Rewrite the handler.** Keep `catalog_revision()` and the `kinds` mapping
exactly as they are. Replace the three `service.*` reads with the watch derivation above.
Delete the `platform_namespaces` field from `CatalogResponse`.

- [ ] **Step 3: Update the web type + fixture.** Remove `platformNamespaces` from
`GlobalResourceCatalogResponse` and from the page test's catalog fixture.

- [ ] **Step 4: Test**

Run: `cargo test -p edgion-center-app --lib api::global_resources` and, in `web/`,
`npm run test -- --run`.
Expected: the new test passes; the CCI-03 watch tests are unaffected.

- [ ] **Step 5: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add -u
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "refactor(global-resources): serve the catalog from the watch registry

Clusters now come from the federation watch cache like the list path,
so the catalog no longer needs the fan-out service. platformNamespaces
had no consumer and is dropped from the response.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Delete the fan-out service, its branches, and the ApiState field

This task is one atomic change: the struct, the field typed against it, the route gate,
the capability plumbing, both bins, and every test literal must move together or nothing
compiles.

**Files:**
- Delete: `crates/center-runtime/src/global_resources/budget.rs`, `crates/center-runtime/src/global_resources/cache.rs`
- Modify: `crates/center-runtime/src/global_resources.rs` — delete everything except the surviving target-resolution core (see Global Constraints); update the module doc to say it now holds only Controller target resolution shared with the planner; delete `mod budget; mod cache;`, the fan-out constants (keep `DEFAULT_MAX_TARGET_CLUSTERS`), `InventoryScanLimits`, `resolve_cluster_targets` (no callers), `InventoryNamespaceError`, `InventoryErrorCode`, `ClusterInventory`, `inventory_error`, `GlobalResourcesInventory`, `ControllerListEnvelope`, `GlobalResourcesService` + its whole impl, `list_cluster_inventory`, `valid_inventory_item`, `valid_resource_name`, `unavailable_cluster_inventory`, `unix_time_ms`, and the now-unused imports; delete the fan-out tests (`cluster_resolution_requires_exactly_one_online_session`, `weak_flight_entries_keep_waiters_together_without_owning_the_flight`, `dropping_inventory_work_drops_all_in_flight_children`, `failed_namespace_discards_partial_pages`, `inventory_item_validation_binds_kind_and_namespace`) and any fixture left with no user; KEEP `standalone_resolution_rejects_a_stale_directory_session`, `kubernetes_resolution_rejects_an_owner_fence_mismatch`, `membership_revision_changes_with_session_and_owner_fence`
- Modify: `crates/center-app/src/api/global_resources.rs` — delete the fan-out tail of `list` (~830-917) and of `detail` (~971-1009); delete `group_inventory` (~328-375), `map_core_error` (~62-74), `service_unavailable` (~54-60); move `PublicInventoryError` + a wire-only `InventoryErrorCode` into this file and drop the `From<&InventoryNamespaceError>` impl; delete the dead `config_data_type_not_applicable` guard (~811-813); shrink `API_KINDS` to the single `edgion-config-data` entry (array type `[ApiKind; 1]`); fix the center-runtime import list; delete the fan-out-only tests `kind_slugs_are_stable_and_core_paths_are_compatible`, `grouping_uses_kind_namespace_name_and_preserves_each_raw_object`, `grouping_filters_config_data_by_exact_nested_type`, `config_data_filter_keeps_every_member_of_a_matching_group`, and the `cluster(...)` helper; REWRITE `inventory_revision_covers_membership_errors_keys_and_raw_objects` to build its groups directly instead of via `group_inventory` (the revision assertion is still wanted)
- Modify: `crates/center-app/src/api/mod.rs` — delete the `global_resources` field (~126-127), the capability downgrade line (~280), and the `&& state.global_resources.is_some()` conjunct in the route gate (~406); delete the service construction in the router test (~1903-1910)
- Modify: the 12 test-state builders that set `global_resources: None` (api/mod.rs, provider_capabilities.rs, global_connection_ip_restriction_handlers.rs, cloudflare_dns.rs, consistency_handlers.rs, audit.rs, roles.rs, region_route_handlers.rs, users.rs, provider_accounts.rs, provider_credential_inspections.rs, global_resources.rs) — grep `global_resources: None` to find them all
- Modify: `bins/edgion-center-standalone/src/cli/mod.rs` — delete the service construction (~310-318) and field assignment (~359); the capability at ~405 becomes `true` unconditionally (the watch path always works); keep the `GlobalResourcesConfig` validation and its use by the planner and store
- Modify: `bins/edgion-center-kubernetes/src/lib.rs` — delete the service construction (~332-339) and field assignment (~385); the capability at ~410 already is `true`
- Test: `crates/center-app/src/api/mod.rs` `mod tests`

**Interfaces:**
- Produces: `ApiState` has no `global_resources`; GlobalResources routes mount on
  `capabilities.global_resources_inventory` alone; `API_KINDS` has one entry.

- [ ] **Step 1: Write the failing test** (router-level, in `api/mod.rs` tests):

```rust
    #[tokio::test]
    async fn global_resources_routes_mount_without_a_fanout_service() {
        // state_with_authz_mode(AuthzMode::AllowAll, false) with
        // capabilities.global_resources_inventory = true.
        // GET /api/v1/center/global-resources/catalog must NOT be 404.
    }
```

It fails today because the gate also requires the service.

- [ ] **Step 2: Apply the deletion.** Do it in this order and let the compiler drive:
delete the runtime service + submodules, then the center-app fan-out branches and DTO
moves, then the `ApiState` field and gate, then the bins, then the test literals. Resolve
every error by deletion, never by re-adding a stub. Run `cargo check --workspace
--all-targets` repeatedly until clean.

- [ ] **Step 3: Verify the surviving exports.** Confirm the planner still compiles against
`request_target`, `resolve_authoritative_targets`, `stable_revision`,
`ClusterResolutionState`, `ResolvedTarget`, `TargetResolutionMode` with no import changes:

Run: `cargo check -p edgion-center-runtime --all-targets` and confirm
`git diff --stat crates/center-runtime/src/global_resource_planner.rs` is empty.

- [ ] **Step 4: Test**

Run: `cargo test -p edgion-center-runtime -p edgion-center-app -p edgion-center-standalone -p edgion-center-kubernetes`
Expected: all pass except the 4 known `global_resource_planner` failures. The five CCI-03
watch tests must pass with only the `global_resources: None` literal removed.

- [ ] **Step 5: Commit**

```bash
git -C /Volumes/ExtStore/ws5/EdgionCenter add -A
git -C /Volumes/ExtStore/ws5/EdgionCenter commit -m "refactor(global-resources): delete the controller HTTP fan-out

GlobalResources is served entirely from the federation watch read model.
The inventory service, its 2s cache, budget, and single-flight machinery
are gone, the ApiState field with them, and routes now mount on the
capability alone. center-core's catalog is deliberately untouched: it is
the durable contract of the GlobalResource sync feature.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Web — shed the five-kind unions

**Files:**
- Modify: `web/src/api/globalResources.ts` — `GlobalResourceKind` (~14-19) → `'EdgionConfigData'`; `GlobalResourceApiSlug` (~21-26) → `'edgion-config-data'`
- Modify: `web/src/api/globalResources.test.ts` — the `'http-route'` detail-path assertion (~22, 36) becomes `'edgion-config-data'`; the `findGlobalResourceDescriptor('/global-resources/http-route')` negative case (~146) stays valid but its comment should say the slug no longer exists
- Modify: `web/src/pages/GlobalResources/GlobalResourceComparisonTable.test.tsx` (~20, 24, 37, 47, 54) and `web/src/pages/GlobalResources/globalResourceConsistency.test.ts` (~10) — fixture `kind: 'HTTPRoute'` → `'EdgionConfigData'` (type-free strings today; this is cosmetic alignment, do it in the same pass)

**Interfaces:**
- Produces: the TS unions match the server's single remaining kind. `InventoryErrorCode`
  and `GlobalResourceClusterError` STAY (the `errors` wire slot is retained by design);
  the legacy-kind redirect routes in `App.tsx` STAY (they keep old bookmarks working).

- [ ] **Step 1: Update the unions and the tests together** (a union shrink breaks the
`'http-route'` fixture immediately — that is the failing signal).
- [ ] **Step 2: Run** `npm run test -- --run` and `npm run build` in `web/`. Expected: clean.
- [ ] **Step 3: Commit** (`refactor(web): narrow GlobalResources to EdgionConfigData` + co-author trailer)

---

### Task 5: Docs and repository verification

**Files:**
- Modify: `crates/center-app/src/api/mod.rs` doc header (~13) if it still lists deleted routes
- Modify: `docs/design/global-resource-management.md` (~44, 48, 51, 53-55) — describe the watch-backed surface and the removal of preflight/fan-out
- Modify: `skills/02-features/access-control.md` (~126, 137-139) — drop `global-resources:diagnose`
- Modify: stale comments in `crates/center-app/src/api/global_resources.rs` that still describe fan-out behavior (the inventory lists ~256-259, ~304-305, ~466-473, ~476-479, ~552-556, ~815-818, ~964-966, ~1313-1319 — check each against the post-deletion code and fix only those that are now wrong)

- [ ] **Step 1: Update the docs above.**
- [ ] **Step 2: Full verification** (cwd repo root, up to 600000 ms per cargo command):
  `cargo fmt --all` && `cargo check --workspace --all-targets` && `cargo clippy --workspace --all-targets` && `cargo test -p edgion-center-runtime -p edgion-center-app -p edgion-center-standalone -p edgion-center-kubernetes`; then in `web/`: `npm run test -- --run` && `npm run build`.
  Known out-of-scope: the 4 pre-existing `global_resource_planner` failures. Trivial
  fmt/clippy fallout → `chore: appease fmt/clippy for cci-04`.
- [ ] **Step 3: Confirm the deletion is complete**

Run: `grep -rn "GlobalResourcesService\|group_inventory\|list_cluster_inventory\|InventoryScanLimits\|resolve_cluster_targets" crates bins --include=*.rs | grep -v target`
Expected: no hits.

- [ ] **Step 4: Report** files changed, checks, deviations. Do not push.
