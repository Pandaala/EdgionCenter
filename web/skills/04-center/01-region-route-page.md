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

Failover actions fan out the same compact patch to every online Controller.
Center concurrently waits for every POST, calculates the dispatch duration,
then allows the federation list/watch stream the same convergence duration
(capped at ten seconds) before returning the aggregate result.

Consistency compares only managed spec fields. Server-owned metadata such as
`resourceVersion`, UID, generation, and status never creates a conflict.
Missing online Controllers are inconsistent.

The warning action lets an operator choose a Controller that has the resource
and synchronize that complete `EdgionConfigData` document to the other online
Controllers. Sync preserves each target's update precondition and strips
server-owned metadata. Controller federation RBAC remains the final authority.

## Validation

- Shared-schema tests cover both override variants and Service-over-Region
  precedence.
- Center runtime tests cover list/watch classification and aggregation.
- Center API tests cover all-success, partial-failure, and source-to-target sync.
- Frontend tests cover missing-controller and metadata-insensitive consistency.
- Two-Controller integration verifies both menus update from watch without the
  retired effective RegionRoute poll.
