import { apiClient } from './client'
import { getActiveControllerId } from '@/utils/proxy'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface RegionDef {
  name: string
  hashRange: [number, number]
  backendEndpoint: string
  tls: boolean
  failoverTo?: string
}

export interface HashCalcConfig {
  algorithm: string
  modulo: number
}

/** ClusterRegionRoute Entry — per-controller data.
 *
 * Two wire formats arrive here:
 *   - Center aggregation (`controllers[ctrlId]` value): uses `pmNamespace`/`pmName`.
 *   - Controller-side endpoint (single controller view): uses `namespace`/`name`.
 * Both are kept optional; consumers should read with fallback
 * `r.pmNamespace ?? r.namespace`.
 */
export interface ClusterRegionRouteEntry {
  pmNamespace?: string
  pmName?: string
  namespace?: string
  name?: string
  myRegion: string
  regions: RegionDef[]
  keyGet: unknown[]
  hashKeyGet?: unknown[]
  hashCalc?: HashCalcConfig
  routeRules: unknown[]
  routeByKeyConfMatch?: { matchMap: Record<string, string> } | null
}

/** Center aggregated ClusterRegionRoute */
export interface CenterClusterRegionRoute {
  namespace: string
  name: string
  controllers: Record<string, ClusterRegionRouteEntry>
}

/** ServiceRegionRoute Entry — per-controller data.
 * See [[ClusterRegionRouteEntry]] for the dual-wire-format rationale.
 */
export interface ServiceRegionRouteEntry {
  pmNamespace?: string
  pmName?: string
  namespace?: string
  name?: string
  clusterPmRef?: { namespace: string; name: string }
  clusterRef?: { namespace: string; name: string }
  regions: Array<{ name: string; failoverTo?: string }>
  refPlugins: Array<string | { kind?: string; namespace?: string; name?: string }>
}

/** Center aggregated ServiceRegionRoute */
export interface CenterServiceRegionRoute {
  namespace: string
  name: string
  clusterRef: { namespace: string; name: string }
  controllers: Record<string, ServiceRegionRouteEntry>
}

/** Consistency check result */
export interface ConsistencyConflict {
  field: string
  values: Record<string, string>
}

export interface ConsistencyResult {
  namespace: string
  name: string
  consistent: boolean
  controllerCount: number
  conflicts: ConsistencyConflict[]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Pick the path prefix based on viewing context, not backend mode:
// - When a specific controller is active (apiClient routes via center proxy
//   to that controller's admin API), the controller's own endpoints use bare
//   paths like `cluster-region-routes`.
// - When not in controller view (Center aggregation pages), Center's endpoints
//   live under `center/`.
function prefix(): string {
  return getActiveControllerId() ? '' : 'center/'
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

export const regionRouteApi = {
  listClusterRegionRoutes: async (): Promise<{ success: boolean; data: CenterClusterRegionRoute[] | ClusterRegionRouteEntry[] }> => {
    const { data } = await apiClient.get(`${prefix()}cluster-region-routes`)
    return data
  },

  listServiceRegionRoutes: async (): Promise<{ success: boolean; data: CenterServiceRegionRoute[] | ServiceRegionRouteEntry[] }> => {
    const { data } = await apiClient.get(`${prefix()}service-region-routes`)
    return data
  },

  clusterRegionRouteFailover: async (
    namespace: string, name: string, regionName: string, failoverTo: string,
  ): Promise<{ success: boolean; data?: { modified: number; failed: number } }> => {
    const { data } = await apiClient.post(`${prefix()}cluster-region-routes/failover`, {
      namespace, name, regionName, failoverTo,
    })
    return data
  },

  serviceRegionRouteFailover: async (
    namespace: string, name: string, regionName: string, failoverTo: string,
  ): Promise<{ success: boolean; data?: { modified: number; failed: number } }> => {
    const { data } = await apiClient.post(`${prefix()}service-region-routes/failover`, {
      namespace, name, regionName, failoverTo,
    })
    return data
  },

  // Center-only
  clusterRegionRoutesConsistency: async (): Promise<{ success: boolean; data: ConsistencyResult[] }> => {
    const { data } = await apiClient.get('center/cluster-region-routes/consistency')
    return data
  },

  serviceRegionRoutesConsistency: async (): Promise<{ success: boolean; data: ConsistencyResult[] }> => {
    const { data } = await apiClient.get('center/service-region-routes/consistency')
    return data
  },

  clusterRegionRouteSync: async (
    sourceControllerId: string, namespace: string, name: string,
  ): Promise<{ success: boolean; data?: { modified: number; failed: number } }> => {
    const { data } = await apiClient.post('center/cluster-region-routes/sync', {
      sourceControllerId, namespace, name,
    })
    return data
  },

  serviceRegionRouteSync: async (
    sourceControllerId: string, namespace: string, name: string,
  ): Promise<{ success: boolean; data?: { modified: number; failed: number } }> => {
    const { data } = await apiClient.post('center/service-region-routes/sync', {
      sourceControllerId, namespace, name,
    })
    return data
  },
}
