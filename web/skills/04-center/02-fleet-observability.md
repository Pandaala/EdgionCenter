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

Controller inventory/history read failures remain visible as a sanitized page
alert, including when cached rows survive a failed refresh. A first inventory
failure is not an empty fleet: the Dashboard shows unknown membership counts and
withholds the resource overview until an inventory snapshot exists. Cached
snapshots remain visible with a stale/incomplete warning; retry recovery clears
that warning.

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
metadata-only keys for restricted dependencies. Capture the route Controller with
`useControllerMutationTarget` and pass it to every list/listKeys request, including
cluster-scoped kinds. The cache key and HTTP target must use the same captured
identity; the mutable global proxy selection is not a request target. Restricted
Secret/ConfigMap inventory pages follow the same rule. It displays these main chains:

```text
GatewayClass -> Gateway -> Route -> Service -> EndpointSlice -> Backend address
HTTPRoute -> EdgionBackend -> credential Secret / quota Redis LinkSys
EdgionBackend -> EdgionBackendTrafficPolicy
Route -> EdgionPlugins / EdgionStreamPlugins -> ConfigData / LinkSys / Secret
Gateway -> EdgionTls / EdgionAcme -> Secret
Service -> BackendTLSPolicy / EdgionBackendTrafficPolicy
```

EdgionBackend is loaded through the normal per-Controller resource endpoint and
placed alongside Services and policies. AI credential edges use only declared
secretRef and redisRef fields; Secret inventory remains metadata-only. Explicit
foreign groups remain unknown, and unavailable inventory stays distinct from a
missing referenced object. These edges describe configuration, not provider calls.

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
Programmed are positive conditions; Conflicted=True and ListenersNotValid=True are errors and
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

BackendTLSPolicy's client certificate option uses the shared policy name parser,
not the generic namespace/name string reference parser. Trim accepted names and
resolve only in the policy namespace. Nonempty malformed strings remain unknown
references, never resolved Secret edges or cross-namespace ReferenceGrant checks.
Empty/non-string values do not invent dependencies; Controller conditions remain
responsible for reporting invalid policy configuration.


ReferenceGrant projections apply to outbound cross-namespace references, not
reverse parent/policy attachment arrows. Route-to-Gateway attachment follows the
Controller's listener allowedRoutes policy; the graph does not independently
reimplement selector evaluation. When ReferenceGrant validation configuration or
the grant inventory is unavailable, show an unknown check rather than a denial.
A resolved structural edge is never an authorization or attachment-success claim.


Gateway uses native top-level addresses, conditions and listeners, not the
ControllerStatus envelope used by GatewayClass and several Edgion kinds.
GatewayStatusDetails renders gateway-level conditions once and each listener's
conditions alongside its attachedRoutes and supportedKinds. Do not flatten
listener conditions into the gateway section as well. Listener freshness is
compared against the Gateway metadata.generation.


Current BackendTLSPolicy and EdgionBackendTrafficPolicy encode arbitration loss
as Accepted=False with reason Conflicted on the ancestor, not as
Conflicted=True/LostOldestWins. Topology reports both rejection and conflict for
that exact current condition; generation freshness still takes precedence.


EndpointSlice backend nodes use the current Gateway discovery eligibility rule:
only explicit `conditions.ready: true` avoids the not-ready badge. Missing/null
ready and serving-only endpoints remain not ready. This is source readiness,
not an active probe result; topology does not infer endpoint runtime health.
