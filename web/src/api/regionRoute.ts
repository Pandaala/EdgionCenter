import { apiClient } from './client'
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
 * Post through the shared write core and always resolve to a
 * `WriteOutcomeSummary`, even on a non-2xx response — the write core's own
 * fan-out endpoints (`failover_response` / `sync_watched_override`) attach
 * `outcomes` on partial (207) AND total failure alike — 409 when every
 * controller rejected the CAS precondition, 502 when nothing landed for any
 * other reason — so the per-controller detail must survive either way.
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

  /**
   * One Controller's own effective RegionRoute view, derived by that Controller
   * from its `EdgionPlugins` entries. Center never aggregates this — the request
   * reaches the selected Controller through the proxy tunnel.
   */
  listRegionRoutes: async (): Promise<{ success: boolean; data: EffectiveRegionRoute[] }> => {
    const { data } = await apiClient.get('region-routes/effective')
    return data
  },

  /**
   * Failover on a single Controller, through its dedicated endpoint.
   *
   * This is the Controller's own handler, not a Center fan-out, so the reply is
   * a flat `{success, modified}` with `modified` a boolean — NOT Center's
   * `{data: {modified, failed}}` envelope. The Controller answers 200 only when
   * it actually patched: a missing override is 404 and a write failure is 500,
   * both of which `apiClient` already turns into a rejection.
   */
  regionRouteFailover: async (
    namespace: string, name: string, regionName: string, failoverTo: string,
  ): Promise<{ success: boolean; modified: boolean }> => {
    const { data } = await apiClient.post('cluster-region-routes/failover', {
      namespace, name, regionName, failoverTo,
    })
    if (!data.success || !data.modified) {
      throw new Error('Failover was not applied on the selected Controller')
    }
    return data
  }
}
