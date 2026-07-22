import { apiClient } from './client'
import type { ApiResponse } from './types'

/** Cloudflare DNS routes are Center-owned and must never reach a Controller proxy. */
const centerRequest = { _skipControllerProxy: true } as any
const centerPath = (path: string) => `/api/v1/center/cloudflare/dns/${path}`

export type CloudflareRecordType = 'A' | 'AAAA' | 'CNAME' | 'TXT' | 'MX' | 'SRV' | 'CAA' | 'NS' | 'SOA'
export type CloudflareTtl = { type: 'automatic' } | { type: 'seconds'; seconds: number }
export type CloudflareProxy = 'dns_only' | 'proxied'
export type CloudflareFlattening = 'provider_default' | 'flatten' | 'do_not_flatten'

export interface CloudflareZone {
  providerAccountId: string
  zoneId: string
  name: string
  kind: 'full' | 'partial' | 'secondary' | 'internal'
  status: 'initializing' | 'pending' | 'active' | 'moved'
  visibility: 'public' | 'private'
  nameservers: string[]
  revision?: string
}

export type CloudflareRecordValue =
  | { type: 'A'; address: string }
  | { type: 'AAAA'; address: string }
  | { type: 'CNAME'; target: string }
  | { type: 'TXT'; segments: Array<{ base64: string }> }
  | { type: 'MX'; preference: number; exchange: string }
  | { type: 'SRV'; priority: number; weight: number; port: number; target: string }
  | { type: 'CAA'; flags: number; tag: string; value: { base64: string } }
  | { type: 'NS'; target: string }
  | { type: 'SOA'; primaryNameServer: string; responsibleMailbox: string; serial: number; refresh: number; retry: number; expire: number; minimum: number }

export interface CloudflareRecordSet {
  providerAccountId: string
  zoneId: string
  zoneApex: string
  zoneVisibility: 'public' | 'private'
  owner: string
  recordType: CloudflareRecordType
  ttl: CloudflareTtl
  values: CloudflareRecordValue[]
  proxy?: CloudflareProxy
  cnameFlattening: CloudflareFlattening
  comment?: string
  tags: string[]
  control: { type: 'manual' } | { type: 'remote'; callerAlias: string } | { type: 'invalid_remote_marker' }
  providerObjectIds: string[]
  revision: string
}

export interface CloudflareRecordPutRequest {
  guard: { type: 'must_not_exist' } | { type: 'match_revision'; revision: string }
  ttl: CloudflareTtl
  values: CloudflareRecordValue[]
  proxy?: CloudflareProxy
  cnameFlattening: CloudflareFlattening
  comment?: string
  tags: string[]
}

interface CursorPage<T> {
  items: T[]
  nextCursor?: string
}

function recordsPath(accountId: string, zoneId: string, type: CloudflareRecordType): string {
  return centerPath(`accounts/${encodeURIComponent(accountId)}/zones/${encodeURIComponent(zoneId)}/record-sets/${type}`)
}

export const cloudflareDnsApi = {
  listZones: async (accountId: string, cursor?: string): Promise<ApiResponse<CursorPage<CloudflareZone>>> => {
    const { data } = await apiClient.get(centerPath(`accounts/${encodeURIComponent(accountId)}/zones`), { ...centerRequest, params: cursor ? { cursor } : undefined })
    return data
  },
  createZone: async (accountId: string, name: string): Promise<ApiResponse<CloudflareZone>> => {
    const { data } = await apiClient.post(centerPath(`accounts/${encodeURIComponent(accountId)}/zones`), { name }, centerRequest)
    return data
  },
  deleteZone: async (accountId: string, zone: CloudflareZone): Promise<void> => {
    await apiClient.delete(centerPath(`accounts/${encodeURIComponent(accountId)}/zones/${encodeURIComponent(zone.zoneId)}`), {
      ...centerRequest,
      data: { expectedRevision: zone.revision, confirmName: zone.name },
    })
  },
  listRecords: async (accountId: string, zoneId: string, cursor?: string): Promise<ApiResponse<CursorPage<CloudflareRecordSet>>> => {
    const { data } = await apiClient.get(centerPath(`accounts/${encodeURIComponent(accountId)}/zones/${encodeURIComponent(zoneId)}/record-sets`), { ...centerRequest, params: cursor ? { cursor } : undefined })
    return data
  },
  putRecord: async (accountId: string, zoneId: string, record: CloudflareRecordSet, request: CloudflareRecordPutRequest): Promise<ApiResponse<CloudflareRecordSet>> => {
    const { data } = await apiClient.put(recordsPath(accountId, zoneId, record.recordType), request, {
      ...centerRequest,
      params: { owner: record.owner },
    })
    return data
  },
  deleteRecord: async (accountId: string, zoneId: string, record: CloudflareRecordSet): Promise<void> => {
    await apiClient.delete(recordsPath(accountId, zoneId, record.recordType), {
      ...centerRequest,
      params: { owner: record.owner },
      data: { expectedRevision: record.revision },
    })
  },
}
