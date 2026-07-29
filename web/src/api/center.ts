import { apiClient } from './client'
import { reloadOutcomeFromError, type ReloadOutcome } from './reloadOutcome'

function safeId(id: string): string {
  return id.replace(/\//g, '~')
}

// ---------------------------------------------------------------------------
// Common types
// ---------------------------------------------------------------------------

/**
 * Freshness of a controller's reported resource counts (`key_count` /
 * `per_kind`), derived by Center from liveness rather than elapsed time:
 * - `fresh`: counts are present and the Controller is online.
 * - `stale`: counts are present but frozen at their last value because the
 *   Controller is offline.
 * - `missing`: the Controller has never pushed a StatsReport.
 */
export type StatsState = 'fresh' | 'stale' | 'missing'

export interface ControllerSummary {
  controller_id: string
  cluster: string
  env: string[]
  tag: string[]
  online: boolean
  // `last_seen_secs_ago` is driven by the fed_sync registry;
  // `stats_updated_secs_ago` by the StatsReport push.
  last_seen_secs_ago?: number | null
  stats_updated_secs_ago?: number | null
  key_count: number | null
  // Per-kind resource counts from the latest StatsReport, keyed by the
  // Controller's own Kind name as pushed on the wire (e.g. "HTTPRoute").
  // `null` until the first StatsReport arrives.
  per_kind?: Record<string, number> | null
  stats_state: StatsState
}

export interface AdminControllerDto {
  controllerId: string
  cluster: string
  env: string[]
  tag: string[]
  online: boolean
  lastSeenAt: number
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

export const centerApi = {
  // ── General ────────────────────────────────────────────────────────────
  listControllers: async (): Promise<{ success: boolean; data?: ControllerSummary[]; count: number }> => {
    const { data } = await apiClient.get('controllers')
    return data
  },
  listClusters: async (): Promise<{ success: boolean; data?: string[]; count: number }> => {
    const { data } = await apiClient.get('clusters')
    return data
  },
  /**
   * Reload one Controller and resolve with its terminal outcome.
   *
   * `_silent` is required: the shared response interceptor pops its own
   * hard-coded error toast for 409/503, which would bury the reload-specific
   * wording (the leader address, the retry hint) under a generic message —
   * the very flattening this outcome exists to undo.
   *
   * A `conflict`/`failed` outcome arrives on a non-2xx status so scripts see
   * an honest failure, so it is lifted out of the rejection and resolved like
   * any other state. Only a genuine transport/auth failure (404 unknown
   * controller, 401, network error) still rejects.
   */
  reloadController: async (id: string): Promise<ReloadOutcome> => {
    try {
      const { data } = await apiClient.post<{ success: boolean; data?: ReloadOutcome }>(
        `controllers/${safeId(id)}/reload`,
        undefined,
        { _silent: true } as never,
      )
      if (!data?.data) throw new Error('reload response carried no outcome')
      return data.data
    } catch (error) {
      const outcome = reloadOutcomeFromError(error)
      if (outcome) return outcome
      throw error
    }
  },
  // ── Admin ──────────────────────────────────────────────────────────────
  listAdminControllers: async (): Promise<{ success: boolean; data?: AdminControllerDto[]; count: number }> => {
    const { data } = await apiClient.get('center/admin/controllers')
    return data
  },
  deleteAdminController: async (id: string): Promise<{ success: boolean }> => {
    const { data } = await apiClient.delete(`center/admin/controllers/${safeId(id)}`)
    return data
  },
}
