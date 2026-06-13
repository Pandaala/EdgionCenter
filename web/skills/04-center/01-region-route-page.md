---
name: center-region-route-page
description: Center federation RegionRoute page design: Global PM page + Service PM page, data fetching strategy, Failover operation, conflict detection.
---

# Center RegionRoute Page Design

## Related Files

| File | Description |
|------|------|
| `src/pages/Center/ClusterRegionRoutePage.tsx` | Cluster PM page |
| `src/pages/Center/RegionRoutePage.tsx` | Service PM page (includes Failover) |
| `src/api/center.ts` | Center API layer |
| `src/components/Layout/CenterLayout.tsx` | Sidebar navigation |

## Page Structure

The RegionRoute sidebar entry has two sub-menus:

```
RegionRoute
├── Cluster  → /region-routes/cluster   (ClusterRegionRoutePage)
└── Service  → /region-routes/service  (RegionRoutePage)
```

## Backend API

### Center Aggregation Endpoints

| Method | Path | Returns |
|------|------|------|
| GET | `center/cluster-region-pms` | `{ namespace, name, controllers[] }[]` |
| GET | `center/service-region-pms` | `{ namespace, name, controllers[] }[]` |
| POST | `center/cluster-region-pms/failover` | `{ namespace, name, regionName, failoverTo }` → `{ modified, failed }` |
| POST | `center/service-region-pms/failover` | Same as above |

### Controller Endpoints (via `proxy/{ctrlId}/api/v1/...`)

| Path | Returns |
|------|------|
| `cluster-region-pms` | Full topology: `{ namespace, name, myRegion, regions[], keyGet[], hashCalc, routeRules[] }` |
| `service-region-pms` | `{ namespace, name, clusterRef: {ns, name}, regions: [{name, failoverTo?}], refPlugins[] }` |

## Cluster PM Page

**Data source**: `center/cluster-region-pms` (main table) + proxy to controller's `cluster-region-pms` (details)

**Columns**: Namespace | Name | Regions

**Expanded row**: Fetches ClusterRegionRoute details from each controller proxy; displays the region topology table, conflict detection, routeRules, and hashCalc.

**RegionsCell**: Fetches a region preview from the first controller (lightweight, one proxy request per row).

## Service PM Page

**Data source**: `center/service-region-pms` (main table) + proxy to controller's `service-region-pms` + `cluster-region-pms` (details)

**Columns**: Service PM Name | Namespace | Controllers | Regions | Failover

**Data fetching optimization**:
- The main table's RegionsCell uses `FirstControllerCtx` (React Context) to fetch all service PM + cluster PM from one controller once, shared across all rows. Only 2 proxy requests per full page load.
- Expanded rows fetch data from all controllers on demand for conflict detection.

```
RegionRoutePage
├── FirstControllerCtx.Provider       ← fetched once, shared via context
│   ├── Table
│   │   ├── RegionsCell               ← looks up from context, no extra requests
│   │   └── RowActions → FailoverPanel  ← gets canonicalRegions + clusterRef from context
│   └── ExpandedDetail                ← fetches from all controller proxies on demand
```

## Failover Operation

Uses Center's fan-out endpoint instead of proxying individual PUT requests to each controller:

```typescript
centerApi.clusterPmFailover(namespace, name, regionName, failoverTo)
// POST center/cluster-region-pms/failover → automatically fans out to all controllers
// returns { modified: N, failed: N }
```

**Flow**:
1. FailoverPanel displays canonicalRegions (fetched from context)
2. User modifies the failoverTo dropdown
3. Click Apply → POST only for changed regions
4. On success, invalidate React Query cache → auto-refresh + close Popover

## Conflict Detection

Compares the regions field of ClusterRegionRoute across controllers:

```typescript
interface RegionConflict {
  regionName: string
  field: 'hashRange' | 'backendEndpoint' | 'failoverTo'
  items: Array<{ controllerId: string; value: string }>
}
```

A conflict is detected when the same region has inconsistent hashRange/endpoint/failoverTo values across different controllers.
Conflicts are displayed as a Warning Alert in the ExpandedDetail expanded row.

## Key Types

```typescript
// center.ts
interface RegionPmSummary { namespace: string; name: string; controllers: string[] }
interface ClusterRegionPmDetail { namespace: string; name: string; myRegion: string; regions: RegionDef[]; keyGet; hashCalc; routeRules; routeByKeyConfMatch }
interface ServiceRegionPmDetail { namespace: string; name: string; clusterRef: { namespace: string; name: string }; regions: Array<{ name: string; failoverTo?: string }>; refPlugins: string[] }
interface RegionDef { name: string; hashRange: [number, number]; backendEndpoint: string; tls: boolean; failoverTo?: string }
```
