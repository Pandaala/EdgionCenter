---
name: center-region-route-page
description: Center RegionRoute override management view.
---

# Center RegionRoute Page

The operator defines routing logic and the safe region topology in the
`RegionRoute` entry of `EdgionPlugins`. Runtime failover state is separate: the
entry's `overrideRef` points to a `RegionRouteOverride` `EdgionConfigData`, and
Center manages those override documents. The page never edits or synchronizes
`EdgionPlugins`.

## There is exactly one override dimension

An earlier design also had a Service dimension (`serviceOverrideRef` ->
`ServiceRegionRouteOverride`, with its own page, endpoints, and read-model map).
Edgion never had that type, so the Service surface was permanently empty and has
been removed.

It is not a missing feature. A `RegionRoute` config lives in an `EdgionPlugins`
object that is attached per HTTPRoute/GRPCRoute rule through an `ExtensionRef`
filter, and each object carries its own `overrideRef`. Per-service failover is
therefore already expressible with `RegionRouteOverride` alone: give a service
its own `EdgionPlugins` object pointing at its own override document. Scope is a
property of which object references a document, not of the document's own type.
Do not reintroduce a second dimension.

## Navigation

```text
RegionRoute → /region-routes/region
```

`/region-routes/service` and `/region-routes/services` redirect here.

The page directly displays `RegionRouteOverride` `EdgionConfigData` resources.
It is not a projection of effective plugins, HTTPRoutes, GRPCRoutes, or backend
Service usage.

## Federation read model

Controllers already list/watch `EdgionConfigData` over federation. Center
classifies each watched resource by `spec.data.type` and keeps one map:

```text
(namespace, name) -> controllerId -> raw EdgionConfigData
```

The map accepts only `RegionRouteOverride` — the classification is a type
allowlist of one, so any other `EdgionConfigData` type is dropped rather than
projected. Full list responses replace one Controller's entries, incremental
watch events update or delete one key, and offline Controllers retain their last
observation until eviction.

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

An open failover editor retains its operation snapshot and outcome list across
watch refreshes. A newly inconsistent row disables further edits without
unmounting the result. Reopening starts from the latest representative document;
only an all-converged operation closes the editor automatically.

Consistency compares only managed spec fields. Server-owned metadata such as
`resourceVersion`, UID, generation, and status never creates a conflict.
Missing online Controllers are inconsistent. The sync action reports two
boundaries explicitly: it cannot create missing resources, and it does not copy
the envelope's enable switch. Operators create missing EdgionConfigData or align
spec.enable in the per-Controller editor. An enable-only difference therefore
remains inconsistent even after successful data synchronization. Disabled
overrides display that base routing applies instead of advertising stored
failover settings as active.

The warning action lets an operator choose a Controller that has the resource
and copy its `spec.data` payload to the other online Controllers, through the
same shared write core. Each target write uses that target's own cached
`resourceVersion` as its CAS precondition; the source document's metadata is
never copied. Controller federation RBAC remains the final authority.
The sync outcome list remains mounted when a refreshed row becomes consistent:
matching current documents does not upgrade an accepted, unknown, or superseded
operation to confirmed convergence. Only the now-unnecessary sync controls hide.

## Validation

- Center runtime tests cover list/watch classification (including that a
  non-`RegionRouteOverride` type is not projected) and aggregation.
- Center API tests cover all-success, partial-failure, source-to-target sync, and
  that failover refuses a document of another `EdgionConfigData` type.
- Frontend tests cover missing-controller and metadata-insensitive consistency.
- Two-Controller integration verifies the menu updates from watch without the
  retired effective RegionRoute poll.
