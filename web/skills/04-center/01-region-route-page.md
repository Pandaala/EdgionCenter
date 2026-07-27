---
name: center-region-route-page
description: Center RegionRoute Region and Service override management views.
---

# Center RegionRoute Pages

The operator defines routing logic and the safe Region topology in the
`RegionRoute` entry of `EdgionPlugins`. Runtime failover state is separate:

- `overrideRef` points to a Region-level `RegionRouteOverride`.
- `serviceOverrideRef` points to a service-specific
  `ServiceRegionRouteOverride`.

The Gateway applies the Region override first and the Service override second.
Neither Center page edits or synchronizes `EdgionPlugins`.

## Navigation

```text
RegionRoute
├── Region   → /region-routes/region
└── Service  → /region-routes/service
```

The Region and Service pages directly display their corresponding
`EdgionConfigData` resources. They are not projections of effective plugins,
HTTPRoutes, GRPCRoutes, or backend Service usage.

## Federation read model

Controllers already list/watch `EdgionConfigData` over federation. Center
classifies each watched resource using `spec.data.type` and maintains two maps:

```text
(namespace, name) -> controllerId -> raw EdgionConfigData
```

The Region map accepts only `RegionRouteOverride`; the Service map accepts only
`ServiceRegionRouteOverride`. Full list responses replace one Controller's
entries, incremental watch events update or delete one key, and offline
Controllers retain their last observation until eviction.

Center must not poll `/api/v1/region-routes/effective` for these pages and must
not aggregate pluginName, alias, entryIndex, routing rules, or service usage.

## Operations

Failover writes `failoverTo` directly onto the identified `EdgionConfigData`
document, once per online Controller, through the shared
`config_data_ops::write_config_data` core: the payload is built from that
Controller's LOCAL watch cache and the write carries a CAS `If-Match`
precondition, so a 409 is terminal and never retried. After a successful
write, Center polls its own local watch cache (never the Controller) every
500ms for a flat 10s budget, watching for two independent signals — the
document's `resourceVersion` leaving the precondition (the watch caught up)
and the requested failover actually being in effect. Each controller's
outcome is one of `converged`, `superseded` (the write landed but was then
overwritten — the response carries the document as last observed),
`accepted` (written, but this replica cannot observe that Controller's
convergence locally), `conflict`, `failed`, or `unknown`. The response
aggregates these into `modified` (`converged`/`superseded`/`accepted`/
`unknown` — the write landed) and `failed` (`failed`/`conflict` — nothing was
applied), alongside the full per-controller `outcomes` list.

Consistency compares only managed spec fields. Server-owned metadata such as
`resourceVersion`, UID, generation, and status never creates a conflict.
Missing online Controllers are inconsistent.

The warning action lets an operator choose a Controller that has the resource
and copy its `spec.data` payload to the other online Controllers, through the
same shared write core. Each target write uses that target's own cached
`resourceVersion` as its CAS precondition; the source document's metadata is
never copied. Controller federation RBAC remains the final authority.

## Validation

- Shared-schema tests cover both override variants and Service-over-Region
  precedence.
- Center runtime tests cover list/watch classification and aggregation.
- Center API tests cover all-success, partial-failure, and source-to-target sync.
- Frontend tests cover missing-controller and metadata-insensitive consistency.
- Two-Controller integration verifies both menus update from watch without the
  retired effective RegionRoute poll.
