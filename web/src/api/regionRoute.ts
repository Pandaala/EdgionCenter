import { apiClient } from './client'
import { getActiveControllerId, getAppMode } from '@/utils/proxy'
import type { WriteOutcome, WriteOutcomeSummary } from './writeOutcome'

export type { WriteOutcome, OutcomeState, WriteOutcomeSummary } from './writeOutcome'

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

export interface RegionRouteSyncTargetResult {
  controllerId: string
  success: boolean
  error?: string
}

export interface RegionRouteSyncResult {
  modified: number
  failed: number
  targets: RegionRouteSyncTargetResult[]
}

export interface RegionRouteOverrideResource {
  apiVersion: string
  kind: string
  metadata: {
    namespace?: string
    name?: string
    resourceVersion?: string
  }
  spec: {
    enable?: boolean
    data: {
      type: 'RegionRouteOverride' | 'ServiceRegionRouteOverride'
      config: {
        regions?: Array<{
          name: string
          failoverTo?: string
          hashRange?: [number, number]
          backendEndpoint?: string
          tls?: boolean
        }>
      }
    }
  }
}

export interface CenterRegionRouteOverride {
  namespace: string
  name: string
  controllers: Record<string, RegionRouteOverrideResource>
}

export interface RegionRouteOverrideListResult {
  success: boolean
  data: CenterRegionRouteOverride[]
  onlineControllerIds: string[]
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

/**
 * Post through the shared write core and always resolve to a
 * `WriteOutcomeSummary`, even on a non-2xx response — the write core's own
 * fan-out endpoints (`failover_response` / `sync_watched_override`) attach
 * `outcomes` on partial (207) AND total (502, "every controller failed")
 * failure alike, so the per-controller detail must survive either way.
 * Rethrows only when the backend never reached the write core at all (e.g.
 * "no online controllers", "override not found") — those responses carry no
 * `outcomes` to recover, so there is nothing to render per controller.
 */
async function postForOutcomes(url: string, body: unknown): Promise<WriteOutcomeSummary> {
  try {
    const { data } = await apiClient.post(url, body)
    return {
      modified: data?.data?.modified ?? 0,
      failed: data?.data?.failed ?? 0,
      outcomes: (data?.data?.outcomes ?? []) as WriteOutcome[],
    }
  } catch (error: any) {
    const responseData = error?.response?.data?.data
    if (Array.isArray(responseData?.outcomes)) {
      return {
        modified: responseData.modified ?? 0,
        failed: responseData.failed ?? 0,
        outcomes: responseData.outcomes as WriteOutcome[],
      }
    }
    throw error
  }
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

export const regionRouteApi = {
  listOverrides: async (
    scope: 'region' | 'service',
  ): Promise<RegionRouteOverrideListResult> => {
    const url = scope === 'region'
      ? 'center/region-route-overrides'
      : 'center/service-region-route-overrides'
    const { data } = await apiClient.get(url)
    return data
  },

  /**
   * RegionRoute failover, region_name form — writes through the shared
   * `write_config_data` core on every online Controller. Resolves to the
   * per-controller `WriteOutcomeSummary` instead of throwing on partial or
   * total failure, so the caller can render all six outcome states; only a
   * pre-flight error (no `outcomes` to recover) still rejects.
   */
  overrideFailover: async (
    scope: 'region' | 'service',
    namespace: string,
    name: string,
    regionName: string,
    failoverTo: string,
  ): Promise<WriteOutcomeSummary> => {
    const url = scope === 'region'
      ? 'center/region-route-overrides/failover'
      : 'center/service-region-route-overrides/failover'
    return postForOutcomes(url, { namespace, name, regionName, failoverTo })
  },

  /**
   * Row-level override sync — copies the source Controller's `spec.data`
   * document to the target Controllers through the shared write core.
   * Resolves to the per-controller `WriteOutcomeSummary` (the backend's
   * `targets[]` field was renamed to `outcomes[]`; this reads the current
   * name) instead of throwing on partial or total failure.
   */
  syncOverride: async (
    scope: 'region' | 'service',
    namespace: string,
    name: string,
    sourceControllerId: string,
    targetControllerIds: string[],
  ): Promise<WriteOutcomeSummary> => {
    const url = scope === 'region'
      ? 'center/region-route-overrides/sync'
      : 'center/service-region-route-overrides/sync'
    return postForOutcomes(url, { namespace, name, sourceControllerId, targetControllerIds })
  },

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

  syncRegionRoute: async (
    route: { namespace: string; pluginName: string; entryIndex: number },
    sourceControllerId: string,
    targetControllerIds: string[],
  ): Promise<{ success: boolean; data: RegionRouteSyncResult }> => {
    try {
      const { data } = await apiClient.post('center/region-routes/sync', {
        ...route,
        sourceControllerId,
        targetControllerIds,
      })
      if (!data.success || data.data?.failed > 0) {
        const details = (data.data?.targets ?? [])
          .filter((target: RegionRouteSyncTargetResult) => !target.success)
          .map((target: RegionRouteSyncTargetResult) => `${target.controllerId}: ${target.error ?? 'failed'}`)
          .join('; ')
        throw new Error(details || 'RegionRoute sync was not applied to every target')
      }
      return data
    } catch (error: any) {
      const response = error?.response?.data
      const details = (response?.data?.targets ?? [])
        .filter((target: RegionRouteSyncTargetResult) => !target.success)
        .map((target: RegionRouteSyncTargetResult) => `${target.controllerId}: ${target.error ?? 'failed'}`)
        .join('; ')
      if (details || response?.error) throw new Error(details || response.error)
      throw error
    }
  },

  // Center-only consistency check
  regionRoutesConsistency: async (): Promise<{ success: boolean; data: ConsistencyResult[] }> => {
    const { data } = await apiClient.get('center/region-routes/consistency')
    return data
  },
}
