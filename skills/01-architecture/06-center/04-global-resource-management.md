---
name: center-global-resource-management
description: Architecture boundary for the read-only federated GlobalResources view across managed clusters.
---

# Global resource management

`GlobalResources` is a **read-only**, non-durable view over cluster-owned namespaced
resources. Center holds no durable desired state for it and exposes no write route: the
durable template plus target-selector model was removed, and the surviving write surfaces
(RegionRouteOverride failover and row-level copy) live under the dedicated RegionRoute
endpoints, not here.

## Surface

Three GET routes, all gated by the single `global-resources:read` permission key:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/center/global-resources/catalog` | Supported kinds and per-Controller cluster rows |
| GET | `/api/v1/center/global-resources/resources/{kind}` | Grouped cross-cluster inventory |
| GET | `/api/v1/center/global-resources/resources/{kind}/{namespace}/{name}` | Exact per-Controller detail |

The live global kind is `EdgionConfigData`. `HTTPRoute`, `GRPCRoute`, `EdgionPlugins`, and
`ReferenceGrant` remain Controller-local resources: Center enters them through a selected
Controller instead of holding a cross-cluster inventory mirror.

The GlobalResources dashboard exposes `IpList`, `KeyList`, `Selector`,
`RequestAccessUrlAllowList`, `ProxyProtocolTrust`, `WafRuleBundle`, `WafPolicy`,
and `Misc` as direct children, each backed by an exact `EdgionConfigData` type filter. `RegionRouteOverride`
remains an EdgionConfigData variant but is operated through the dedicated RegionRoute views.

## Read model

All three routes are served entirely from the in-memory federation watch cache, with **zero
Controller HTTP on the read path**. Center does not persist observed resource payloads and
does not merge same-named cluster objects into one mutable object.

Rows are emitted **per Controller, not per cluster**. The `cluster` field is derived by
taking the prefix of a `<cluster>/<suffix>` watch controller id — a convention, not an
enforced invariant: registration does not require the separator, and an id without one falls
back to the whole id. Two Controllers in the same cluster therefore produce two rows carrying
the same `cluster` and different `controllerId`, and a single comparison group may hold
members from both. Detail narrows to one Controller by first match on the
requested cluster; that is selection, not authoritative-target resolution — the resolution
machinery that once backed it was removed with the durable sync feature.

Consequently `ClusterResolution.candidates` is always empty and the `Ambiguous` /
`Indeterminate` states are never constructed on this path. The `errors` array on
`ClusterResult` / `DetailResponse` is likewise always empty; it stays on the wire shape only
because the dashboard drawer renders it. When extending this surface, do not assume those
fields carry signal — populate them deliberately or read them as absent.

Namespace scoping is **not** a Center policy. Center applies no namespace predicate: the
visible set is whatever each Controller streams under its own `watch_namespaces`. The former
`global_resources.platform_namespaces` config field was removed together with the durable
sync feature; there is no configurable platform-namespace set in Center any more.

Failures surface as HTTP error codes from the handler, not as per-cluster error entries:
`invalid_global_resource_kind`, `invalid_query`, `invalid_cluster`, `invalid_limit`,
`invalid_config_data_type`, `invalid_continue_token`, `stale_continue_token`,
`cluster_required`, `cluster_not_found`, and `global_resource_not_found`.

A **watch-level** Controller RBAC denial *is* observable, just not through `errors`: the
Controller answers the watch with `Forbidden`, Center treats it as terminal and marks the
cache stale, and this API then reports that Controller as `offline` with `syncState: "stale"`
and `complete: false`. What is invisible is **per-object** filtering — a single resource the
Controller declines to stream is simply absent from the cache with no signal.

### ConfigData payload visibility

The global read model passes through IpList, KeyList, Selector,
RegionRouteOverride, ProxyProtocolTrust, and WafPolicy configurations. WafRuleBundle
contains private rule/phrase content, RequestAccessUrlAllowList can contain
sensitive condition values, and Misc is arbitrary; their config payloads are
removed from global responses. Unknown type strings also redact by default.
These rows still expose identity/type and Controller membership. Payload inspection
uses the authorized Controller-local storage resource API. Global comparisons
must not infer content equality from two redacted documents.

## Responsibility boundary

Center owns aggregation, presentation, Center authorization, and audit.

Controller remains authoritative for federation identity authorization, resource CRUD,
optimistic concurrency, schema validation, reference resolution, status, ConfCenter
persistence, requeue, and Gateway synchronization. Gateway remains the runtime enforcement
boundary.

Writes elsewhere in Center use the generic Controller Admin HTTP proxy. They add no
resource-specific Controller endpoint, ResourceKind, or federation protobuf message; the
federation Command channel (apply/delete/reload) has been removed entirely, so this proxy is
the only path.

## EdgionConfigData ownership — there is none to negotiate

Mirrored from Edgion, which is canonical:
`../Edgion/skills/01-architecture/05-resources/15-edgion-config-data.md` (Ownership) and
`../Edgion/skills/04-review/architecture/edgion-config-data-managed-by-is-inert.md`.

`EdgionConfigData` is the **Center-controlled data overlay** and is deliberately not an ops-repo
artifact. The git-owned half of Edgion's model is the base config (`EdgionPlugins` and friends),
which carries the logic; the overlay carries only data and exists to be pushed and hot-swapped by
Center. The kind therefore has a **single intended writer**, which is why no ownership arbitration
exists on either side of the federation boundary.

Two consequences bind Center work:

- **Do not rely on, read, or stamp `edgion.io/managed-by`.** The label is inert in Edgion: no
  handler consults it, and the `managed_by` constants in
  `../Edgion/edgion-resources/src/constants/labels.rs` have no callers at all. Its only live use is
  provenance on `Secret` and the ACME-generated `EdgionTls`, written by ACME (`acme`) and the
  conf-sync CA (`conf-sync-ca`) via their own local literals. Putting it on an `EdgionConfigData`
  changes no behavior and misrepresents a guarantee that does not exist.
- **Do not reintroduce label-based ownership.** Center stamping `managed-by: center`, refusing to
  take over an unowned object, and surfacing an `unowned-conflict` drift state was the
  `tasks/pending/global-resource-management/` design, **superseded and rejected on 2026-07-26** by
  `tasks/pending/center-controller-interaction-convergence/`. Write safety is a CAS property
  instead — `If-Match` on `metadata.resourceVersion` with explicit `converged` / `superseded` /
  `conflict` outcomes — and an ownership epoch beside it would be a second, coarser concurrency
  model with nothing to protect.

`superseded` is consequently an expected outcome, not an anomaly: it means the write landed and
something later replaced it. It does not imply a competing authority that Center should have
out-ranked.

Note that the Controller's CAS precondition is opt-in for *other* Admin API callers, so a caller
sending neither `If-Match` nor a body `resourceVersion` still performs an unconditional replace.
That is a Controller-side Admin API contract question, tracked separately in Edgion; it is not an
ownership question and needs no Center-side mechanism.

## Watch constraints

`FedWatchRequest` carries a single `kind` and a `from_version`; `from_version = 0` means
"full list then watch", with no namespace scoping in the protocol. Center keeps **one active
watch per Controller session**, for `EdgionConfigData`. The request issued at registration
resumes from the cached sync version rather than always starting at 0, and Center re-watches
from 0 when the Controller's `server_id` changes or after a transient error.

Bounding is Center-side only — a per-Controller entry cap, with an overflowing batch dropped
whole. Overflow is terminal for that watch: it does not re-watch on its own, so the
Controller stays stale until it re-registers.

**Generic multi-kind reverse-watch multiplexing is an optional later optimization; the
current Controller keeps one active reverse-watch task.** Read this before adding a second
watched kind.

## Scope

"Global" does not mean all Controller resources or arbitrary Kubernetes objects. `Secret` is
permanently absent. Resources such as routes, plugins, and grants retain Controller-local
navigation and authorization. A raw `ConfigMap` is excluded because its data can contain
credentials.
