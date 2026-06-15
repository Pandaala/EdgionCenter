import { apiClient } from './client'
import type { ListResponse } from './types'

/**
 * Audit-log record as returned by the Center read API
 * (`GET /api/v1/center/admin/audit-logs`). Field names are camelCase to match
 * the backend `AuditRecordDto` serde convention.
 */
export interface AuditRecordDto {
  ts: number
  actor: string
  provider: string
  method: string
  path: string
  targetController?: string | null
  status: number
  sourceIp?: string | null
  requestId?: string | null
  detail?: string | null
}

/** Query parameters accepted by the audit-log read endpoint. */
export interface AuditListParams {
  limit?: number
  offset?: number
  actor?: string
  controller?: string
  /** Inclusive lower bound on `ts` (unix seconds). */
  since?: number
  /** Inclusive upper bound on `ts` (unix seconds). */
  until?: number
}

export const auditApi = {
  list: async (params: AuditListParams = {}): Promise<ListResponse<AuditRecordDto>> => {
    // Drop empty/undefined params so we don't send blank query values.
    const query: Record<string, string | number> = {}
    if (params.limit != null) query.limit = params.limit
    if (params.offset != null) query.offset = params.offset
    if (params.actor) query.actor = params.actor
    if (params.controller) query.controller = params.controller
    if (params.since != null) query.since = params.since
    if (params.until != null) query.until = params.until
    const { data } = await apiClient.get('center/admin/audit-logs', { params: query })
    return data
  },
}
