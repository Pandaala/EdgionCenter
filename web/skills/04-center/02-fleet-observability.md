---
name: center-fleet-observability
description: Multi-Controller resource inventory, consistency, condition health, and topology behavior.
---

# Fleet observability

There is no federation diagnostics page. Watch ownership, sync versions, and metadata-store
key coverage are not exposed over the Admin API; the endpoints that once served them
(`/api/v1/center/admin/watch-status`, `/api/v1/center/admin/metadata-store`) were removed
together with the page. Do not reintroduce them without a live consumer.

## Data boundaries

The Center controller list reports connection and aggregate-count metadata. Detailed
resource inventory is read through each online Controller's existing HTTP proxy. The web
loads every first-class catalog resource with the Controller id encoded in the proxy
path and `_skipControllerProxy` set, so comparison never depends on the currently
selected Controller.

Failures are partial: one denied or old resource endpoint is recorded as unavailable
for that Controller/kind and must not hide successful snapshots. Secret and ConfigMap
contents are not loaded by fleet observability.

## Dashboard resource overview

The Center Dashboard is an inventory summary, not a diagnostics console. `ResourceOverviewPanel`
renders entirely from the `ControllerSummary.per_kind` counts already present in
`GET /api/v1/controllers` — each Controller pushes its own per-kind counts to Center via
`StatsReport` over fed_sync, and Center persists them; the panel makes zero network calls of
its own and never proxies to Controllers. It shows Controller membership plus the total number
of resource instances, broken down by kind, summed across every online Controller whose
`stats_state` is not `missing`. A resource reported by two Controllers counts as two fleet
instances; the Dashboard does not deduplicate or compare resource identities. Offline
Controllers (`stats_state: 'stale'`) are excluded from the sum even though Center still holds
their last-reported counts.

Every first-class kind remains visible when its count is zero. If any online Controller has
`stats_state: 'missing'` (it has never pushed a StatsReport), the Dashboard cannot know that
Controller's contribution, so it renders the fleet total and any nonzero per-kind count with a
trailing `+`, and a zero per-kind count as `—`, instead of presenting the numbers as complete.
Drift, unresolved references, rejected conditions, file conflicts, certificate expiry, watch
ownership, and metadata-store coverage are not Dashboard metrics.

## Consistency semantics

The comparison identity is `kind/namespace/name` (`_cluster` for cluster scope). A row
is consistent only when every online Controller has the resource and its operator
document is equal. Status, resourceVersion, uid, generation, managed fields, creation
timestamps, and catalog-declared runtime fields do not participate in the fingerprint.
Conditions remain visible beside the configuration result.

Fleet summaries include counts by kind, cluster, and Controller; missing/divergent
resources; rejected and unresolved conditions; explicit conflict diagnostics; and
ACME certificates expiring within 30 days.

## Topology semantics

The selected-Controller topology loads all first-class relationship resources and
metadata-only keys for restricted dependencies. It displays these main chains:

```text
GatewayClass -> Gateway -> Route -> Service -> EndpointSlice -> Backend address
Route -> EdgionPlugins / EdgionStreamPlugins -> ConfigData / LinkSys / Secret
Gateway -> EdgionTls / EdgionAcme -> Secret
Service -> BackendTLSPolicy / EdgionBackendTrafficPolicy
```

Reference-like fields are resolved using their declared kind and namespace. A missing
target becomes a red unresolved placeholder instead of silently dropping the edge.
Rejected/conflicting conditions are projected onto nodes and edges. Namespace filters
retain connected cluster-scoped parents and cross-namespace dependencies.

## Conditions

Every catalog entry with `hasConditions` uses `ResourceConditions` in its list and
read-only detail. The component collects resource, parent, ancestor, and listener
condition locations and displays type, status, reason, message, observed generation,
transition time, and context. Pages must never synthesize an `Active` status.


Condition colors follow their type's polarity: Accepted, ResolvedRefs and
Programmed are positive conditions; Conflicted=True is an error and
PartiallyInvalid=True is a warning. False clears those negative conditions.
Unknown status stays cautionary, and unrecognized condition types remain neutral
instead of inferring success or failure from their boolean value.

When both metadata.generation and a condition's observedGeneration are safe
nonnegative integers, an older observation is marked `stale` in gold. Pass the
resource generation into every list/detail caller, including Gateway listeners.
The detail shows both generations; compact tooltips explain the mismatch.
Each deployment observation is checked separately. Missing generation information
does not establish staleness. Stale reference conditions do not carry the current
reference-granted/denied test selectors. The topology uses the same freshness rule:
older observations add a stale-status badge and do not contribute current rejection,
conflict or unresolved-reference flags. Current observations from other writers
remain visible. PartiallyInvalid=True has its own warning, and node badges coexist
rather than hiding each other by priority. A condition reporting unresolved refs is
separate from a missing-resource placeholder, so it never suppresses graph edges.
A resolved edge establishes reference existence, not runtime readiness.
