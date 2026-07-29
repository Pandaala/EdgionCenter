# Global resource management

## Status

> **Superseded (2026-07-26).** The durable desired-state half of this design
> (`GlobalResource`, plan/apply synchronization, ownership labels, drift/retry —
> GR-06 through GR-09) was rejected together with
> `tasks/pending/global-resource-management/`; none of it is implemented, and the
> label-ownership model it assumed was explicitly declined. What ships today is
> only the read path: a live GlobalResources inventory served from the federation
> watch read model. Current authority:
> `docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md`
> and `skills/01-architecture/06-center/04-global-resource-management.md`.
> This document is kept as the 2026-07-24 design baseline record only.

This document records the 2026-07-24 design baseline. Implementation was ordered in
`tasks/pending/global-resource-management/03-subtasks.md` (since rejected, see above).

## Product model

Center will expose two related capabilities:

- **GlobalResources** is a live, non-durable fleet view over selected namespaced resources in platform namespaces. The default namespaces are `edgion-system` and `edgion-global`.
- **GlobalResource** is durable Center-owned desired state that can be planned and synchronized to selected clusters.

The live GlobalResources inventory contains only `EdgionConfigData` from configured platform
namespaces. The dashboard exposes `IpList`, `KeyList`, `Selector`, and `Misc` as direct
navigation leaves. `RegionRouteOverride` is managed through RegionRoute views; `HTTPRoute`,
`GRPCRoute`, `EdgionPlugins`, and `ReferenceGrant` remain Controller-local.

These names do not change Kubernetes scope. Every listed object remains namespaced.

## Control boundary

Center organizes cluster selection, aggregation, comparison, desired state, synchronization plans, operation status, authorization, and audit. It does not become a second Edgion Controller.

Every read and mutation goes through one target Controller. The Controller remains responsible for:

- federation identity authorization,
- resource schema validation,
- resourceVersion concurrency,
- ConfCenter persistence,
- reference resolution,
- status conditions,
- requeue and Gateway synchronization.

Center never writes directly to a managed cluster's Kubernetes API.

## First delivery

The first delivery uses the existing federation HTTP proxy and Controller resource CRUD API. It needs no resource-specific Controller endpoint or federation protobuf change.

GR-06 adds durable desired-state CRUD and a read-only plan endpoint. The plan resolves authoritative Controller targets, normalizes only root Kubernetes-owned fields, and returns bounded evidence without applying changes. Desired-state replacement requires a strong generation `If-Match`; apply is reserved for GR-07 and must use the Controller resource body's `metadata.resourceVersion` as the CAS value.

Inventory reads for `EdgionConfigData` are served entirely from the in-memory federation
watch read model — there is no Controller HTTP call on the read path — and do not persist
observed payloads. Synchronization is manual plan-and-apply. It creates missing objects or
updates matching Center-owned objects with a fresh resourceVersion precondition.

The read-only Admin API is:

- `GET /api/v1/center/global-resources/catalog`
- `GET /api/v1/center/global-resources/resources/{kind}`
- `GET /api/v1/center/global-resources/resources/{kind}/{namespace}/{name}?cluster=...`

All three routes require `global-resources:read`. There is no separate preflight route or
`global-resources:diagnose` permission: every route reads only `EdgionConfigData` from the
watch read model, so there is no bounded Controller reachability probe left to gate
separately. List continuation tokens are opaque and bind the exact query, watch cache
membership, inventory snapshot, and configured catalog revision.

The first delivery does not:

- select arbitrarily between multiple Controllers advertising one cluster,
- overwrite an unowned object,
- replay an ambiguous write,
- automatically reconcile drift,
- delete or prune target objects,
- inventory or synchronize Secret.

## Drift and retry policy (GR-09)

Drift is computed by requesting a fresh detail object and comparing it with the persisted desired revision after root cluster-owned fields are normalized. The dashboard may refresh this plan on demand; it must not label a missing or incomplete observation as an empty or in-sync result.

Manual apply retries no target automatically. HTTP 409 is a conflict, 401/403 is denied, 429 is rate-limited, and a timeout, connection loss, or response loss is unknown because the write may have reached Controller. Unknown outcomes require a new plan and explicit user confirmation. A future continuous reconciler must use the same generation, membership, ownership, resourceVersion, and bounded-deadline fences, with exponential backoff and no automatic replay of unknown writes.

## Persistence

Observed cluster payloads are not stored. GlobalResource desired state and operation records are stored because restart recovery, audit, target history, and drift require durable intent.

Standalone uses SQL through the SQL adapter. Kubernetes mode uses a dedicated Center CRD and Kubernetes-native coordination. The two compositions retain their dependency isolation.

## Authorization

Center authorization and Controller federation authorization both apply.

Controller's built-in Center policy already permits reads of the initial kinds, but it permits writes only for `EdgionConfigData`. Synchronizing `HTTPRoute`, `GRPCRoute`, `EdgionPlugins`, or `ReferenceGrant` requires explicit concrete create/update grants. Wildcard reads are not acceptable because they can expose Secret.

## Open decisions

- How a cluster declares one authoritative Controller when multiple Controllers advertise the same cluster.
- Exact Center ownership and desired-revision labels or annotations.
- Kubernetes GlobalResource CRD schema and reconciliation Lease.
- Future adoption and prune behavior.
- Whether measured inventory load justifies generic multi-kind reverse-watch multiplexing.

## Local release readiness

The local development topology has two ready Controller/Gateway pairs, one in each platform namespace, with Center running locally against MySQL. Vite serves the dashboard at `http://127.0.0.1:5173/` and proxies `/api` to Center's `12201` Admin API. No generic Controller transport change is included until measurements show that the existing bounded requests are insufficient.
