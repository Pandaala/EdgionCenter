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

export interface RegionRouteBackendService {
  namespace: string
  name: string
  port?: number
}

export interface RegionRouteServiceUsage {
  routeKind: 'HTTPRoute' | 'GRPCRoute' | string
  routeNamespace: string
  routeName: string
  ruleIndex: number
  backendServices: RegionRouteBackendService[]
}

export interface RegionRouteOverrideRef {
  namespace: string
  name: string
  permitted: boolean
}

/** Per-controller effective region route view. */
export interface EffectiveRegionRoute {
  namespace: string
  pluginName: string
  alias: string | null
  entryIndex: number
  myRegion: string
  regions: RegionDef[]
  keyGet: unknown[]
  hashKeyGet?: unknown[]
  hashCalc?: { algorithm?: string; modulo?: number; [key: string]: unknown }
  routeRules: Array<{ type?: string; [key: string]: unknown }>
  routeByKeyConfMatch?: Record<string, unknown>
  dye?: unknown
  overrideRef: RegionRouteOverrideRef | null
  overrideApplied: boolean
  serviceUsages: RegionRouteServiceUsage[]
}

/** Center aggregated region route — one row per (namespace, pluginName, alias) tuple. */
export interface CenterRegionRoute {
  namespace: string
  pluginName: string
  alias: string | null
  entryIndex: number
  controllers: Record<string, EffectiveRegionRoute>
  /** Online fleet membership, emitted under the same region-routes:read permission. */
  onlineControllerIds?: string[]
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
    route?: { pluginName: string; entryIndex: number },
  ): Promise<{ success: boolean; data?: { modified: number; failed: number } }> => {
    const center = prefix() === 'center/'
    const url = center ? 'center/region-routes/failover' : 'cluster-region-routes/failover'
    const { data } = await apiClient.post(url, {
      namespace, name, regionName, failoverTo,
      ...(center && route ? { pluginName: route.pluginName, entryIndex: route.entryIndex } : {}),
    })
    if (!data.success || (data.data?.failed ?? 0) > 0 || (data.data?.modified ?? 0) === 0) {
      throw new Error(`Failover was not applied to every target (${data.data?.modified ?? 0} modified, ${data.data?.failed ?? 0} failed)`)
    }
    return data
  },

  // Center-only consistency check
  regionRoutesConsistency: async (): Promise<{ success: boolean; data: ConsistencyResult[] }> => {
    const { data } = await apiClient.get('center/region-routes/consistency')
    return data
  },
}
