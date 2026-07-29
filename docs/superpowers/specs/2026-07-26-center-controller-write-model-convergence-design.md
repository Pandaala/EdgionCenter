# Center–Controller Write-Model Convergence

| Key | Value |
|-----|-------|
| Date | 2026-07-26 |
| Status | approved |
| Repositories | EdgionCenter, Edgion |
| Relation | Authoritative spec for the `tasks/pending/center-controller-interaction-convergence/` (CCI) program. Section 7 defines the complete task breakdown (CCI-00 – CCI-10); the task files under `tasks/` mirror it. Supersedes the original CCI-08 operation model. |

## 1. Context and goals

This spec is the single authoritative definition of the Center–Controller interaction
convergence program. It converges all communication on two product flows — the federation
`EdgionConfigData` list/watch read model and the on-demand HTTP proxy into one selected
Controller — with registration/heartbeat/stats remaining a compact status projection.
The end state:

- **Read has exactly one path**: the federation watch cache.
- **Write has exactly one path**: the generic Controller Admin resource CRUD over the proxy
  tunnel, guarded by CAS preconditions.
- **No dedicated channels remain**: the Controller failover endpoints, the synthetic
  `RegionRoute` authz kind, and the federation Command channel are removed.
- The five communication defects that block or undermine this model are fixed first.

### End-state inventory of every communication surface

The unified target: everything between Center and Controller rides the single mTLS
`FederationSync.Sync` stream, carrying exactly four flows — session control
(register/heartbeat), one read path (watch), one write/drill-in path (proxy CRUD), and one
status feed (stats). Every other surface is deleted or folded into those four.

| Surface | Disposition | Owner |
|---|---|---|
| `RegisterRequest` / `RegisterAck` handshake, SPIFFE binding, session takeover | **Keep unchanged** | — |
| `Ping` / `Pong` heartbeat (Center-driven, 30 s, 3-miss budget) | **Keep unchanged** | — |
| `FedWatch*` reverse watch of `EdgionConfigData` | **Keep — becomes the only read path** for global config; payload type-filtering and read-model hardening | CCI-02 |
| `StatsReport` (5 s coalesced counts) | **Keep — becomes the only inventory source**; surface `per_kind` via the API | CCI-05, F4 |
| `HttpProxyRequest/Response` tunnel | **Keep — becomes the only write and drill-in path**, hardened by F1–F3 | this spec |
| `CommandRequest/Response` channel (Reload/Apply/Delete) | **Delete**; Reload rewired through the proxy tunnel | this spec §4 |
| Dedicated Controller failover endpoints + synthetic `RegionRoute` authz kind | **Delete**; failover becomes generic ConfigData PUT | this spec §3, §5 |
| RegionRoute base-table remnants (`region_routes` map, effective/consistency/sync endpoints, Controller `GET /api/v1/region-routes/effective`, empty stubs) | **Delete** — Center manages overrides only; base config belongs to business teams via `EdgionPlugins` | this spec §5 |
| GlobalConnectionIpRestriction effective-state 10 s poller + metadata projection | **Delete** (replaced by watch-based read model) | CCI-09 |
| GlobalResources five-kind fan-out listing | **Delete** (replaced by the watch-backed GlobalResources API) | CCI-03, CCI-04 |
| Dashboard per-controller N×20-kind list fan-out for counts | **Delete** (replaced by `StatsReport.per_kind`) | CCI-05 |
| Legacy admin endpoints (`watch-status`, `metadata-store`), 308 redirects, dead config/code remnants | **Delete** | CCI-09, this spec §5 |
| `RegisterRequest.supported_kinds` (sent, shape-validated, never consumed) | **Delete** — the watch is pinned to `EdgionConfigData`, which every Controller serves, so negotiation has no consumer; the field also ships unfiltered `no_sync` kinds. Proto tag stays a hole; re-add only when a multi-kind watch actually negotiates | CCI-01 |
| Center↔Center `InternalForwarding` (Kubernetes replica hop) | **Keep**; drop only the `ForwardCommand` RPC with the Command channel | this spec §4 |

### Non-goals

- Implementing `ServiceRegionRouteOverride` in Edgion (resources enum, CRD,
  `serviceOverrideRef`, gateway two-level merge). That is a separate data-plane feature with
  its own spec. This spec's write model applies to it unchanged once it exists; the task
  breakdown marks the few items blocked on it.
- Federation protobuf changes beyond deleting the Command-channel messages (and, if CCI-01
  decides to drop it, the `supported_kinds` field).
- Direct Center access to managed-cluster Kubernetes APIs.

## 2. Prerequisite fixes

These land before (or together with) the write-model change. F1 is a hard prerequisite:
without it, DELETE has no concurrency protection at all.

| # | Fix | Anchor |
|---|-----|--------|
| F1 | Add `if-match` to the Controller federation proxy request-header allowlist. Today the Center forwards it (`center-app/src/api/mod.rs:78`) but the Controller drops it (`Edgion edgion-controller/src/fed_sync/fed_client/mod.rs:44`), so `expected_resource_version` falls back to the body; PUT survives via the body `resourceVersion`, DELETE (body `"{}"`) becomes unconditional. | `fed_client/mod.rs:44` |
| F2 | Align size limits. Center federation gRPC server: set `max_decoding_message_size` to 16 MiB (above the Controller's 10 MiB proxy-response cap) in both Center binaries; today the tonic 4 MiB default kills the whole stream on a large list response. Center proxy route: add `DefaultBodyLimit` of 1 MiB to match the Controller's `MAX_PROXY_REQUEST_BODY` 413 threshold. | `bins/edgion-center-standalone/src/cli/mod.rs`, `bins/edgion-center-kubernetes/src/lib.rs`, `center-app/src/api/mod.rs:393` |
| F3 | Lower the Center forward timeout default (`sync.command_timeout_secs`) from 30 s to 25 s so a slow Controller surfaces as a Center 504 instead of a browser abort (the dashboard axios timeout is 30 s). | `center-runtime/src/federation/config.rs:14` |
| F4 | In `controller_summaries`, fall back to the in-process aggregator for `key_count` and `stats_updated_secs_ago` when the directory record has no stats (standalone SQL mode persists only `last_seen_at`, so these are structurally null today). | `center-app/src/api/mod.rs:227-232` |
| F5 | Percent-decode the request path in the audit middleware before extracting the target controller id, so an encoded id cannot be audited under a different string than the one dispatched. | `center-app/src/common/audit/middleware.rs:64-74` |

## 3. Write model (replaces the CCI-08 core)

### Read side

Mutations construct their write payload from the watch cache entry of the **same
Controller** being written (`ns/name -> controller_id -> raw document`, including
`metadata.resourceVersion`). There is **no pre-write fresh GET**. Correctness does not
depend on cache freshness: a stale document fails the CAS precondition and is rejected with
409; it can never silently overwrite. Row-level source-to-target sync remains the one
explicit cross-controller copy operation and keeps its preview/confirm flow (CCI-08
boundary retained).

### Write side

All writes go through the generic proxy CRUD, each carrying a CAS precondition:

- Create: `POST /api/v1/namespaced/edgionconfigdata/{ns}`.
- Update: full-document `PUT /api/v1/namespaced/edgionconfigdata/{ns}/{name}` with
  `If-Match` and the body `resourceVersion` taken from the cache document.
- Delete: `DELETE .../{ns}/{name}` with `If-Match` (enabled by F1).
- A write without a version precondition is a bug; the Center refuses to send it.

Failover becomes an ordinary update: patch `failoverTo` in the cached document client-side
and PUT the whole document. Before writing, compare against the cache and skip the write if
the target state already holds (replicating the old endpoint's `AlreadyApplied` idempotent
short-circuit).

### Outcomes

The CCI-08 outcome model is retained: `accepted` on 2xx, `converged` only after a newer
matching watch observation (existing `wait_for_override_projection` mechanism, 10 s cap),
`unknown` on timeout or response loss, never auto-retried. New handling for conflicts:

- 409 marks the operation `conflict`. No automatic retry. The UI tells the user the view was
  stale; the watch catches up and the user re-confirms on the refreshed document.

### Validation

Semantic checks (e.g. the referenced region exists) run in the Center/UI before the write.
A bad document that slips through is contained by the Controller's lenient admission
(`Accepted=False` status condition) and the gateway's fail-to-base merge; it cannot corrupt
the data plane.

### Chattiness

The per-mutation live re-fetch of `/api/v1/access` (`web/src/api/resources.ts:73-87`) is
replaced by the existing `useControllerAccess` cache (10 s staleTime), eliminating the 2N
tunnel round trips of `batchDelete`.

## 4. Reload migration and Command-channel removal

Reload moves onto the proxy tunnel as `POST /api/v1/reload` (the Controller's Command
handler already just calls that route in-process). The Controller default Center RBAC policy
gains a `Verb::Reload` grant on the non-resource route, fixing the current
always-500 behavior (`default_policy` today denies it).

The Command channel is then deleted end to end, in both repositories:

- Center: `Commander`, `PendingCommandGuard`, the `ForwardCommand` internal RPC, and the
  reload handler's Command dispatch (rewired to `ProxyForwarder`).
- Controller: the `CenterPayload::Command` handler arm.
- Proto (both copies, kept byte-aligned): `CommandRequest`, `CommandResponse`,
  `ApplyCommand`, `DeleteCommand`; the freed oneof tags are left as holes, never reused.

## 5. Deletion inventory (extends CCI-09)

> **Amendment 2026-07-27 (supersedes the Controller RegionRoute rows below).** The
> Controller-perspective RegionRoute view is **retained**: an operator still needs to inspect
> and fail over a single Controller independently, and the dashboard reaches it by drilling
> into a Controller (`/controller/{id}/region-routes`), which proxies to that Controller's own
> endpoints. `GET /api/v1/region-routes/effective`,
> `POST /api/v1/cluster-region-routes/failover`, and the synthetic `RegionRoute` authz kind
> with `Verb::Failover` therefore stay. The Center-side deletions in this section were carried
> out in full. See `tasks/new_work/06-delete-controller-region-route-surface.md`.

### Edgion (Controller)

- `POST /api/v1/cluster-region-routes/failover` and
  `POST /api/v1/service-region-routes/failover`, with `patch_edgion_config_data_failover`,
  `patch_failover_resource`, and the per-resource lock usage they own
  (`api/region_route_handlers.rs`). CAS on the generic PUT now serializes writers.
- The deprecated always-empty stubs `GET /api/v1/cluster-region-routes` and
  `GET /api/v1/service-region-routes`.
- `GET /api/v1/region-routes/effective` (documented as "for the Center poller", which no
  longer exists; does a four-kind aggregation per call).
- The synthetic `RegionRoute` authz kind: classifier entries, `Verb::Failover`, and the
  default-policy rules and rationale comments that reference the removed routes.
- The never-read `CenterClientConfig.ping_interval_secs` config field.

### EdgionCenter

- `CenterMetaDataStore.region_routes` map, `replace_region_routes{,_fenced}`, and every
  reader (all permanently empty in production): `GET /api/v1/center/region-routes`, the
  region-routes consistency handler, `POST .../region-routes/sync`, the
  `plugin_name`+`entry_index` failover form, and the `MetaDataStoreStatus.region_routes`
  diagnostic. Remove the two contradictory feed comments (`metadata_store.rs:192-194`,
  `poll.rs:92-93`).
- `direct_override_failover` / `fan_out_failover` calls to the dedicated Controller
  endpoints, rewired to the generic PUT flow of section 3.
- `parse_region_effective`, `poll_controller_once_fenced`, and the
  `ControllerSummary.last_list_secs_ago` compatibility field (plus its dashboard fallback
  branch).
- Stale sample-config keys `sync.list_interval_secs` / `sync.list_timeout_secs` in
  `config/edgion-center.yaml`.
- The outdated review doc `skills/04-review/architecture/fed-proxy-header-forwarding.md`
  (argues for forwarding full header maps; strict allowlists shipped since).
- Web callers of every removed endpoint (`web/src/api/regionRoute.ts` and pages), and the
  stale `federation-diagnostics` e2e assertions (already covered by CCI-09's admin-endpoint
  removals; listed here because the failover UI rewiring touches the same files).

## 6. RBAC end state

The Controller's built-in Center policy collapses to three rules:

1. Read (`get`/`list`/`watch`/`list-keys`) on all resource kinds except `Secret`.
2. Write (`create`/`update`/`delete`) on `EdgionConfigData`.
3. Non-resource: `server-info` and `reload`.

The synthetic `RegionRoute` kind disappears entirely. Center-side proxy authorization is
unchanged here; its read/write key split landed separately (see the CCI-07 entry in §7).

## 7. Task breakdown (CCI-00 – CCI-10)

This section is the complete program plan. The files under
`tasks/pending/center-controller-interaction-convergence/tasks/` mirror these definitions;
where they diverge, this spec wins. Execution order: CCI-00 first (independent, smallest);
then the numbered order. Items marked **[SRRO-blocked]** wait for the separate Edgion
`ServiceRegionRouteOverride` spec; everything else proceeds without it.

### CCI-00 — Federation transport prerequisite fixes (new)

Both repositories. No dependencies; lands first. Implements section 2 minus F4:
F1 (`if-match` in the Controller proxy allowlist), F2 (gRPC/body size alignment),
F3 (25 s forward timeout), F5 (audit path percent-decode).

### CCI-01 — Audit and harden the ConfigData federation watch; remove the Command channel

Both repositories. Reuses the implemented `EdgionConfigData` list/watch; no namespace
filtering added to the protocol.

- Delete `RegisterRequest.supported_kinds` end to end (field, Controller-side
  computation, Center-side validation/storage); the proto tag stays a hole. Decision
  recorded in the section 1 inventory: the watch is pinned to `EdgionConfigData`, so
  negotiation has no consumer.
- Handle terminal watch errors (`Forbidden`, unsupported kind) without infinite retry.
- Verify full list, incremental events, `from_version` resume, reconnect, and
  ConfigSyncServer restart (`ServerReload`) behavior.
- Guarantee one active watch per Controller session.
- Delete the Command channel end to end and rewire Reload through the proxy tunnel with a
  default-policy `Verb::Reload` grant (section 4).

### CCI-02 — Extend and harden the ConfigData read model

EdgionCenter. Reuses `CenterWatchCache`; no second cache.

- Extend the projection from the two override types to all ConfigData types: `IpList`,
  `KeyList`, `Selector`, `Misc`, `RegionRouteOverride`, and **[SRRO-blocked]**
  `ServiceRegionRouteOverride`.
- Identity is `controller_id + namespace + name`.
- Batch atomicity (a malformed list/event must not be partially applied and still advance
  the version), version-gap detection, disconnect staleness marking, capacity limits.
- Expose revision and type-change information (`subscribe_changes()`). Originally built for
  CCI-06's SSE stream; CCI-06 was dropped, and the broadcast is retained as the optional
  wakeup source for CCI-08's convergence wait.
- Secrets and credential-bearing payloads never enter the global read model; `Misc` bodies
  are metadata-only in the global projection.

### CCI-03 — GlobalResources on the ConfigData read model

EdgionCenter. GlobalResources reads the CCI-02 cache directly.

- Show only `EdgionConfigData`, in the four categories `IpList`, `KeyList`, `Selector`,
  `Misc`; no fixed platform-namespace filtering — namespace is identity and operation
  context only.
- List and detail views make no Controller HTTP calls; rows carry Controller, cluster,
  sync state, freshness, and revision.
- Mutations follow section 3: the write payload comes from the watch-cache document with a
  CAS precondition — **no pre-edit fresh GET** (this corrects the earlier CCI-03 draft).

### CCI-04 — Remove the five-kind GlobalResources fan-out

EdgionCenter. Delete the old GlobalResources kinds (`HTTPRoute`, `GRPCRoute`,
`EdgionPlugins`, `ReferenceGrant`), the request-time Controller list fan-out, the 2 s
inventory cache, single-flight/fan-out-budget/pagination conversion, and the old
preflight/diagnostics/permission/frontend branches. These kinds remain manageable in the
selected-Controller drill-in pages.

### CCI-05 — Dashboard via StatsReport

EdgionCenter. Dashboard counts come from `StatsReport`, never from listing resource
objects.

- Surface `per_kind` and totals through the controllers API; Controller list and Dashboard
  share one status source.
- Delete the `ResourceOverviewPanel` object aggregation and
  `useControllerResourceSnapshots`.
- Distinguish stats-missing, stats-stale, and true-zero (requires F4: aggregator fallback
  for standalone SQL mode, section 2).

### CCI-06 — Browser revision push (SSE) — DROPPED (2026-07-27)

Not implemented. The original design (Center→browser SSE carrying resource type,
controller, and revision — no bodies) is sound but does not earn its cost:

- **The correctness case is already covered.** Because writes use the watch-cache document
  plus a CAS precondition (section 3), acting on a stale read cannot lose an update — the
  write 409s. Stale reads are a display inconvenience, not a data hazard.
- **Staleness is already visible.** CCI-03 surfaces per-row `syncState` and freshness;
  CCI-05 surfaces `stats_state`. Knowing the data is stale was the larger half of the
  value, and it shipped.
- **Your own writes already converge.** CCI-08 reports `converged` only after the watch
  observes the change, so the operator sees the truth after their own edit.
- **The remaining window is narrow.** GlobalResources is now EdgionConfigData only
  (IpList / KeyList / Selector / Misc) — low-churn global configuration, behind a manual
  Refresh button.

Cheaper substitute if the staleness ever bites: lower `staleTime` for the GlobalResources
queries and enable `refetchOnWindowFocus` (both are global React Query defaults in
`web/src/main.tsx`), which covers the dominant "switched tabs and came back" case without
a streaming endpoint or its operational surface.

Reopen this only if the dashboard becomes a always-on monitoring surface or concurrent
multi-operator editing becomes routine. CCI-09 has nothing to preserve here; CCI-10 drops
its SSE validation item. The `subscribe_changes()` broadcast built in CCI-02 is retained —
CCI-08's convergence wait may consume it (see that entry).

### CCI-07 — Selected-Controller on-demand CRUD and authorization — FOLDED INTO CCI-09 (2026-07-27)

Not run as a separate task. The original entry proposed building a Center-side permission
taxonomy (read / write / sensitive) plus a Center-side validator for HTTP method, path, and
resource kind. Verification against both repositories showed that machinery already exists,
in two layers that answer different questions:

- **Controller → Center is a capability grant.** `center.rbac` decides what the Center, as a
  single SPIFFE identity, may do on that Controller; `authz_classifier.rs` already maps
  method + path to `(Verb, kind)` across all 16 route shapes and default-denies anything
  unmatched, and `default_policy.rs` evaluates it per verb per kind. The built-in default is
  deliberately fail-closed (writes only on `EdgionConfigData`); an operator who wants
  drill-in writes grants them explicitly. `/api/v1/access` reports the *effective* policy for
  the caller's identity, so the dashboard's buttons follow the operator's grant with no
  Center-side kind table.
- **Center → its users is a distribution decision, and Center owns it.** In Kubernetes mode
  the SubjectAccessReview for a proxied request already carries the concrete path and HTTP
  verb (`sar.rs`), so native Kubernetes RBAC expresses per-verb and per-kind grants today —
  e.g. `nonResourceURLs: ["/edgion-center-authz/api/v1/proxy/*"], verbs: ["get"]` is a
  read-only fleet operator. Adding Center permission keys would layer a second, coarser model
  on top of a working one.

The sensitive-kind rules the entry asked for are already enforced, and more strictly: the
Controller's default policy denies `Secret` entirely and leaves `ConfigMap` and
`EndpointSlice` read-only. "Ordinary Controller pages establish no background watch" is
already true — no drill-in page has an interval refetch or subscription.

Two items survive and move to CCI-09:

1. Delete the synthetic `RegionRoute` authz kind and `Verb::Failover` — the only authorization
   shape that does not fit the standard resource model, and dead since CCI-08 made failover a
   generic ConfigData PUT. Already in CCI-09's inventory.
2. Remove the per-mutation live `/api/v1/access` re-fetch in the dashboard (six call sites;
   `batchDelete` loops the single delete, so deleting N resources costs N extra access
   round trips). The cached access model is authority for presentation; the Controller
   remains authority for the write.

Closed 2026-07-29 (`tasks/new_work/12`): `SqlAuthorizer::authorize` matches only the
permission key and ignores `request_path` / `request_verb`, so standalone deployments could
not express a read-only proxy grant the way Kubernetes mode can. Resolved with the one extra
catalog key this entry anticipated, and no new model: `proxy:access` is replaced by
`proxy:read` (`GET`/`HEAD`) and `proxy:write` (everything else). Per-Controller and per-kind
scoping stays dropped — see
`skills/04-review/architecture/center-proxy-permission-method-split-only.md`.

### CCI-08 — Unified operation model for ConfigData writes

EdgionCenter. Implements section 3 exactly: watch-cache document + CAS (PUT with
`If-Match` + body `resourceVersion`; DELETE with `If-Match`; POST for create), the
`AlreadyApplied` idempotent skip, 409 → `conflict` with no automatic retry, and the
`accepted` → `converged` / `failed` / `unknown` outcome model with watch-observation
convergence. Covers ConfigData create/update/delete, Selector active-profile switching,
RegionRoute failover, **[SRRO-blocked]** ServiceRegionRoute failover, and explicit
row-level source-to-target sync (preview/confirm, no durable desired state, no ambiguous
identities).

#### Convergence wait — design decided 2026-07-27

Replaces both mechanisms in today's `region_route_handlers`: the blind
`sleep(region_route_propagation_wait(elapsed))` at two call sites (always pays, verifies
nothing) and `wait_for_override_projection`'s field-content polling. The constants
`REGION_ROUTE_PROPAGATION_WAIT_MIN` / `_LIMIT` and that helper are deleted.

1. **Poll the local watch cache, never the Controller.** A 500 ms tick over an in-memory
   lookup. Polling the Controller would rebuild the fan-out CCI-04 deleted and fight the
   watch model. (Event-driven wakeup via `subscribe_changes()` was considered and rejected:
   for a human-initiated, low-frequency operation the sub-second gain does not pay for
   broadcast `Lagged` handling and multi-source select. Revisit only if measured latency
   justifies it.)

2. **Two independent signals, reported separately.** Neither alone answers the operator's
   question.

   | Signal | Proves |
   |---|---|
   | cached `metadata.resourceVersion` differs from the CAS precondition | the watch has caught up to at least this write |
   | the operation's own field matches what was written (e.g. `failoverTo`) | the intent is *currently* in effect |

   Why "version moved" is sound here despite resourceVersion being bumped by unrelated
   Kubernetes activity (status subresource writes, other actors): the precondition is read
   from the watch cache and the write is a CAS. Any change landing between read and write
   makes the write 409 — the wait is never reached. A successful CAS therefore pins the
   write to that exact version, and cached versions only move forward (a re-watch resync
   returns current state), so leaving the precondition value can only mean advancing to at
   or past this write. Verified prerequisite: the watch payload does carry
   `metadata.resourceVersion` — the Controller caches full typed objects and
   `MetadataFilterConfig` strips only blocked annotations and `managedFields`.

3. **Outcomes.** version moved + field matches → `converged`. Version moved + field does
   not match → report explicitly that the write took effect and was then superseded, and
   return the observed current value; do not collapse this into a timeout. Version
   unchanged until the budget expires → `unknown`. The budget stays a generous flat 10 s:
   the loop returns the moment it observes convergence, so generosity is free, and an
   adaptive budget would only change how fast a broken path gives up.

4. **Measure, don't guess.** Record the observed convergence latency as telemetry so the
   real distribution is known per cluster. It is deliberately *not* an input to the
   timeout.

5. **Multi-replica ownership.** In Kubernetes the replica serving the HTTP request may not
   hold that Controller's federation session, and the watch cache is process-local — such
   a replica can never observe convergence. Detect non-ownership and return `accepted` with
   that reason immediately instead of burning the budget and reporting a misleading
   non-convergence. (This gap exists today, masked by the 10 s timeout's warning.)

### CCI-09 — Remove obsolete interaction surfaces

Both repositories. Executes section 5 in full. Center side: GlobalConnectionIpRestriction
pages/APIs and its effective-state poller, the Desired State & Sync page and
`global_resource_sync` persistence/planner, old metadata projections, RegionRoute
aggregate/consistency APIs, Federation Diagnostics remnants, `watch-status` /
`metadata-store` admin endpoints, legacy redirects, and the associated permissions,
capabilities, routes, translations, and tests. Edgion side: both dedicated failover
endpoints with their patch helpers, the two always-empty RegionRoute list stubs,
`GET /api/v1/region-routes/effective`, the synthetic `RegionRoute` authz kind with
`Verb::Failover`, and the dead `ping_interval_secs` config field. Retained: the ConfigData
watch read model, the `subscribe_changes()` broadcast, failover **as generic ConfigData
PUT**, row-level sync, and selected-Controller CRUD.

#### Selector active-profile switch — decided 2026-07-27

No longer retained. The dedicated
`PATCH /api/v1/center/global-connection-ip-restrictions/{ns}/{name}/active-profile` fan-out
has been **removed**, and with it the GlobalConnectionIpRestriction dashboard pages. Switching
a profile is a write of `/spec/data/config/active` on a Selector `EdgionConfigData`, which the
generic ConfigData write path already performs with the same CAS precondition and the same
convergence outcomes — the dedicated endpoint was a second spelling of one operation, and its
only remaining reason to exist was the fleet-wide effective-state poller that CCI-09 deletes.

Center-side management of these shared plugins is dropped for this program rather than
re-sourced. A future design may reintroduce an operator-facing switch; if it does, it starts
from the generic ConfigData surface, not from the deleted handler. Consequence for the Edgion
side: `GET /api/v1/global-ip-restrictions/effective` (`effective_handlers.rs`) loses its last
consumer and is deleted with the RegionRoute surface.

Absorbed from CCI-07 (see that entry for why the rest of it was dropped): remove the
dashboard's per-mutation live `/api/v1/access` re-fetch at its six call sites, so the cached
access model is the only presentation authority and `batchDelete` stops paying one extra
access round trip per deleted resource. The Controller stays the authority for the write
itself — this changes what the UI asks, not what is enforced.

### CCI-10 — Integration, performance, docs, and release validation

Both repositories. Validate the final architecture with two Controllers: initial full
list; add/update/delete events; multi-namespace identity; Controller disconnect,
reconnect, and eviction; ConfigSyncServer restart; version gap and full resync; watch
denied/unsupported/overflow; ordinary-resource drill-in CRUD; ConfigData CRUD (including
writing a Selector's `active` through the generic path, now that the dedicated switch is
gone); both failovers (**[SRRO-blocked]** for the service variant), and row-level sync; the
convergence-wait outcomes of CCI-08 (`converged`, superseded-after-write, `unknown`, and
the non-owning-replica `accepted` path). Plus the section 8 behavior tests. Final proofs: the
Dashboard never lists Controller resources; GlobalResources performs no HTTP fan-out; idle
pages issue no recurring Controller requests; docs and Skills describe only the final
architecture.

**Verification must run `cargo test` in BOTH repositories.** Earlier tasks in this program
verified Edgion with `check` / `clippy` / doc-validation only, which let a stale assertion in
`edgion-controller/src/authz/access.rs` sit red from the CCI-01 reload grant until it surfaced
during CCI-07 scoping (fixed in Edgion commit `1a86f054`). A cross-repository change is not
verified until both suites have run.

### Program-document notes

- The original CCI-08 task file stays `completed (superseded)`; its replacement definition
  is the CCI-08 entry above.
- The stale Edgion user docs describing the removed
  `ServiceRegionRoute -> ClusterRegionRoute` chain (`docs/{en,zh-CN}/user-guide/http-route/
  filters/edgion-plugins/region-route.md`, `skills/01-architecture/02-gateway/
  05-plugin-system.md:168`) are rewritten in the ServiceRegionRouteOverride spec, not here.

## 8. Validation

- Per-repository local checks: Edgion `cargo fmt` / `check --workspace --all-targets` /
  `clippy` plus its doc-consistency guards; EdgionCenter's local checks and the affected
  web e2e specs (update, don't skip, the removed-endpoint assertions).
- Behavior tests:
  - DELETE through the proxy with a stale `If-Match` returns 409; without `If-Match` the
    Center refuses to send.
  - PUT conflict path: stale cache document -> 409 -> no auto-retry -> refreshed document
    succeeds.
  - A >4 MiB proxied list response completes without tearing down the federation stream.
  - Reload via the proxy succeeds under the default policy.
  - Failover via generic PUT reaches `converged` through the watch observation, and the
    idempotent short-circuit skips a no-op write.
- Cross-repository: proto copies stay byte-aligned after the Command-message removal
  (existing round-trip test plus a diff in CI or review).
