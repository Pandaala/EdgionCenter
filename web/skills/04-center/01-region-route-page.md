---
name: center-region-route-page
description: Center RegionRoute Region and Service multi-cluster management views.
---

# Center RegionRoute Pages

RegionRoute is an HTTP request plugin embedded in `EdgionPlugins`. Its complete routing logic and
safe default region table are git-owned. An optional `overrideRef` points to a
`RegionRouteOverride` `EdgionConfigData` that is the only Center-writable failover surface.

Preserve the historical two-dimensional Center management design without restoring the removed
`ClusterRegionRoute` or `ServiceRegionRoute` persistence models. Both views are projections of the
current effective RegionRoute contract.

## Navigation

```text
RegionRoute
├── Region   → /region-routes/region
└── Service  → /region-routes/service
```

`/region-routes` redirects to the Region page. The former `/cluster`, `/topology`, and `/services` URLs remain
redirect aliases. A selected Controller keeps the compact
`/region-routes` route because the split is a Center fleet projection.

## Effective contract

Center polls each Controller's `GET /api/v1/region-routes/effective` endpoint and aggregates by
`(namespace, pluginName, entryIndex)`. `entryIndex` is the stable position in `requestPlugins` and
prevents missing or duplicate aliases from overwriting another entry. Each Controller entry includes:

- `myRegion`, `regions`, `keyGet`, `hashKeyGet`, `hashCalc`, `routeRules`, and
  `routeByKeyConfMatch`, and `dyeHeaders`;
- structured `overrideRef { namespace, name, permitted }` and `overrideApplied`; `regions` is the
  effective whole-replacement overlay when a permitted, enabled `RegionRouteOverride` resolves;
- `serviceUsages`, derived by the Controller from HTTPRoute/GRPCRoute ExtensionRefs targeting the
  containing EdgionPlugins resource.

The contract is additive and defaults new collections to empty so a rolling upgrade can accept an
older Controller without dropping its effective RegionRoute row.
Each aggregated row also carries `onlineControllerIds`, resolved by the Center backend under the
same `region-routes:read` permission. The Service page must show unknown coverage when an older
backend omits this membership instead of treating only reporting Controllers as the complete fleet.

## Region page

The Region page shows one row per aggregated RegionRoute plugin entry. Expanded rows show the
complete routing configuration for every Controller. Consistency checks compare shared routing
logic, effective regions, and override state. They report missing online Controllers as a
`presence` conflict. `myRegion` and `serviceUsages` are intentionally local deployment state and
are not cross-cluster consistency conflicts.

Failover writes only `RegionRouteOverride.regions[].failoverTo`. Center resolves each online
Controller's own structured reference, including cross-namespace targets, rather than copying an
arbitrary Controller's reference across the fleet. Zero-target, partial, and all-failed writes are
reported as failures. The action is disabled when the plugin has no permitted `overrideRef`; the
Center must never rewrite the git-owned base plugin.

## Service page

The Service page groups each logical usage across Controllers instead of rendering duplicate rows.
It shows Controller coverage, backend and effective-region consistency, and per-Controller expanded
details. A usage records its Route kind, namespace/name, zero-based rule index, and Service backends
from that rule. Rule-level ExtensionRefs
apply to all Service backends in the rule; backend-level ExtensionRefs apply only to that backend.
Delegated HTTPRoute/GRPCRoute trees use Controller-produced `resolvedRules`, so inherited parent
ExtensionRefs are attributed to the child Service backends that actually receive traffic.

ExtensionRefs are namespace-local. Cross-namespace or non-`edgion.io/EdgionPlugins` references are
not attributed to a RegionRoute plugin. Failover is not Service-local in the current schema: it is
stored in the shared RegionRoute override and affects all consumers. Service rows therefore link to
the Region management action and must not imply an isolated per-Service write.

## Validation

- Controller unit tests cover rule-level, backend-level, delegated usage discovery, structured
  references, and effective overlay application.
- Center runtime tests cover additive deserialization and multi-Controller aggregation.
- Frontend tests cover distinct navigation and both views.
- Kubernetes E2E must contain two Controllers and an HTTPRoute that references the RegionRoute
  fixture, assert both Controllers appear in Region and Service, execute a failover across
  both Controllers, verify labels are preserved, and restore the original overlay state.
