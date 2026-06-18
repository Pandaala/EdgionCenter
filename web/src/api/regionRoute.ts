import { apiClient } from './client'
import { getActiveControllerId, getAppMode } from '@/utils/proxy'

// ---------------------------------------------------------------------------
// Types — match the FROZEN backend contract (camelCase on the wire)
// ---------------------------------------------------------------------------

export interface RegionDef {
  name: string
  hashRange: [number, number]
  backendEndpoint: string
  tls: boolean
  failoverTo?: string
}

/** Per-controller effective region route view. */
export interface EffectiveRegionRoute {
  namespace: string
  pluginName: string
  alias: string | null
  myRegion: string
  regions: RegionDef[]
  overrideRef: string | null
  overrideApplied: boolean
}

/** Center aggregated region route — one row per (namespace, pluginName, alias) tuple. */
export interface CenterRegionRoute {
  namespace: string
  pluginName: string
  alias: string | null
  controllers: Record<string, EffectiveRegionRoute>
}

export interface ConsistencyResult {
  namespace: string
  name: string
  consistent: boolean
  controllerCount: number
  /** Field names that differ across online controllers, e.g. ["regions"]. */
  conflicts: string[]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Path prefix based on viewing context:
 * - Center aggregated view (mode=center, no active controller): 'center/'
 * - Controller proxy view (active controller set): ''
 * - Standalone controller view (mode=controller): ''
 */
function prefix(): string {
  if (getActiveControllerId()) return ''
  if (getAppMode() === 'controller') return ''
  return 'center/'
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

export const regionRouteApi = {
  listRegionRoutes: async (): Promise<{ success: boolean; data: CenterRegionRoute[] | EffectiveRegionRoute[] }> => {
    const center = prefix() === 'center/'
    const url = center ? 'center/region-routes' : 'region-routes/effective'
    const { data } = await apiClient.get(url)
    return data
  },

  regionRouteFailover: async (
    namespace: string, name: string, regionName: string, failoverTo: string,
  ): Promise<{ success: boolean; data?: { modified: number; failed: number } }> => {
    const center = prefix() === 'center/'
    const url = center ? 'center/region-routes/failover' : 'cluster-region-routes/failover'
    const { data } = await apiClient.post(url, {
      namespace, name, regionName, failoverTo,
    })
    return data
  },

  // Center-only consistency check
  regionRoutesConsistency: async (): Promise<{ success: boolean; data: ConsistencyResult[] }> => {
    const { data } = await apiClient.get('center/region-routes/consistency')
    return data
  },
}
