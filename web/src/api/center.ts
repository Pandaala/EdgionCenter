import { apiClient } from './client'

function safeId(id: string): string {
  return id.replace(/\//g, '~')
}

// ---------------------------------------------------------------------------
// Common types
// ---------------------------------------------------------------------------

export interface ControllerSummary {
  controller_id: string
  cluster: string
  env: string[]
  tag: string[]
  online: boolean
  // Legacy field retained for backwards compatibility with older Center
  // builds. New builds emit `last_seen_secs_ago` (driven by the fed_sync
  // registry) and `stats_updated_secs_ago` (driven by StatsReport push).
  last_list_secs_ago?: number | null
  last_seen_secs_ago?: number | null
  stats_updated_secs_ago?: number | null
  key_count: number | null
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
  reloadController: async (id: string): Promise<{ success: boolean }> => {
    const { data } = await apiClient.post(`controllers/${safeId(id)}/reload`)
    return data
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
