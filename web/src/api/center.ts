import { apiClient } from './client'
import type { K8sResource, ListResponse, ResourceKind, ResourceScope } from './types'

function safeId(id: string): string {
  return id.replace(/\//g, '~')
}

export function controllerResourcePath(id: string, kind: ResourceKind, scope: ResourceScope): string {
  return `/api/v1/proxy/${safeId(id)}/api/v1/${scope === 'cluster' ? 'cluster' : 'namespaced'}/${kind}`
}
export function controllerDiagnosticsPath(id: string): string {
  return `/api/v1/proxy/${safeId(id)}/api/v1/diagnostics/conf-conflicts`
}
export type ControllerConfConflict = { kind: string; key: string; winner: string; losers: string[] }
export function parseControllerConfConflicts(value: unknown): { conflicts: ControllerConfConflict[] } {
  const record = value as { conflicts?: unknown } | null
  if (!record || !Array.isArray(record.conflicts)) throw new Error('Invalid conflict diagnostics response')
  const conflicts = record.conflicts.map((item) => {
    const entry = item as Partial<ControllerConfConflict>
    if (typeof entry.kind !== 'string' || typeof entry.key !== 'string' || typeof entry.winner !== 'string'
      || !Array.isArray(entry.losers) || !entry.losers.every((loser) => typeof loser === 'string')) {
      throw new Error('Invalid conflict diagnostics entry')
    }
    return entry as ControllerConfConflict
  })
  return { conflicts }
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

export interface WatchControllerStatus {
  controllerId: string
  syncVersion: number
  serverId: string
}

export interface MetadataStoreEntry {
  key: string
  controllerCount: number
}

export interface MetadataStoreStatus {
  regionRoutes: MetadataStoreEntry[]
  globalConnectionIpRestrictions: MetadataStoreEntry[]
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
  listControllerResources: async (
    id: string,
    kind: ResourceKind,
    scope: ResourceScope,
  ): Promise<ListResponse<K8sResource>> => {
    const path = controllerResourcePath(id, kind, scope)
    const { data } = await apiClient.get(path, {
      _skipControllerProxy: true,
      _silent: true,
    } as any)
    return data
  },
  controllerConfConflicts: async (id: string): Promise<{ conflicts: ControllerConfConflict[] }> => {
    const { data } = await apiClient.get(controllerDiagnosticsPath(id), { _skipControllerProxy: true, _silent: true } as any)
    return parseControllerConfConflicts(data?.data ?? data)
  },
  watchStatus: async (): Promise<{ success: boolean; data?: WatchControllerStatus[]; count: number }> => {
    const { data } = await apiClient.get('center/admin/watch-status')
    return data
  },
  metadataStoreStatus: async (): Promise<{ success: boolean; data?: MetadataStoreStatus }> => {
    const { data } = await apiClient.get('center/admin/metadata-store')
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
