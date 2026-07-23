import { apiClient } from './client'
import type { ApiResponse } from './types'

/** Route 53 routes belong to Center and must not be sent through a Controller proxy. */
const centerRequest = { _skipControllerProxy: true } as any
const centerPath = (path: string) => `/api/v1/center/aws/route53/${path}`

export type Route53RecordType = 'A' | 'AAAA' | 'CNAME' | 'TXT' | 'MX' | 'SRV' | 'CAA' | 'NS' | 'SOA'

export interface Route53Zone {
  providerAccountId: string
  zoneId: string
  apex: string
  visibility: 'public' | 'private'
}

export interface Route53RecordSet {
  providerAccountId: string
  zoneId: string
  zoneApex: string
  zoneVisibility: 'public' | 'private'
  recordSet: {
    key: { owner: string; recordType: Route53RecordType; routing: Route53RoutingIdentity }
    ttl: { type: 'inherited' } | { type: 'seconds'; seconds: number }
    values: Route53RecordValue[]
    extension?: Route53RecordExtension
  }
  control: 'external_or_manual'
  revision: string
}

/** Exact `DnsRoutingIdentity` serde projection returned by the Route 53 read API. */
export type Route53RoutingIdentity = { type: 'simple' } | { type: 'route53'; set_identifier: string }

/** Exact `DnsRecordSetValue` serde projection returned by the Route 53 read API. */
export type Route53RecordValue =
  | { type: 'A'; address: string }
  | { type: 'AAAA'; address: string }
  | { type: 'CNAME'; target: string }
  | { type: 'TXT'; value: number[][] }
  | { type: 'MX'; preference: number; exchange: string }
  | { type: 'SRV'; priority: number; weight: number; port: number; target: string }
  | { type: 'CAA'; flags: number; tag: string; value: number[] }
  | { type: 'NS'; target: string }
  | { type: 'SOA'; primary_name_server: string; responsible_mailbox: string; serial: number; refresh: number; retry: number; expire: number; minimum: number }

/** Exact `DnsRecordExtension` Route 53 projection: it is tagged by `provider`, not `type`. */
export interface Route53RecordExtension {
  provider: 'route53'
  alias_target?: Route53AliasTarget
  routing_policy?: Route53RoutingPolicy
  health_check_id?: string
}

export interface Route53AliasTarget {
  targetZoneId: string
  target: string
  evaluateTargetHealth: boolean
}

export type Route53RoutingPolicy =
  | { type: 'weighted'; weight: number }
  | { type: 'failover'; role: 'primary' | 'secondary' }
  | { type: 'latency'; region: string }
  | { type: 'geolocation'; location: { type: 'default' } | { type: 'continent'; code: string } | { type: 'country'; code: string } | { type: 'us_subdivision'; code: string } }
  | { type: 'multivalue' }

/** Write-only `Route53RecordValueDto` shape; it deliberately differs from read values. */
export type Route53RecordWriteValue =
  | { type: 'A'; address: string }
  | { type: 'AAAA'; address: string }
  | { type: 'CNAME'; target: string }
  | { type: 'TXT'; segments: Array<{ base64: string }> }

export interface Route53RecordDesired {
  ttl: { type: 'inherited' } | { type: 'seconds'; seconds: number }
  values: Route53RecordWriteValue[]
  aliasTarget?: Route53AliasTarget
  routingPolicy?: Route53RoutingPolicy
  healthCheckId?: string
}

export interface Route53ChangeReceipt {
  receipt: string
  providerApplication: 'PENDING' | 'IN_SYNC'
  authoritativeConvergence: 'not_checked'
}

export interface Route53ZoneLifecycleObservation {
  zone: Route53Zone
  revision: string
  authoritativeNameservers: string[]
  delegation: { state: string; expectedNameservers: string[]; parentNameservers: string[]; checkedAt?: string; failure?: string }
  readiness: 'awaiting_authoritative_verification' | 'ready' | 'verification_failed'
  dnssec: { state: string; dsRecords: Array<{ keyTag: number; algorithm: number; digestType: number; digest: string }>; externalAction: unknown; providerDetail?: string }
  nonDefaultRecordCount: number
}

export interface Route53ZoneLifecycleMutation {
  mutationId: string
  providerApplication: 'pending' | 'accepted'
  authoritativeConvergence: 'not_checked'
}

interface CursorPage<T> { items: T[]; nextCursor?: string }

function zonePath(accountId: string, zoneId?: string): string {
  const root = `accounts/${encodeURIComponent(accountId)}/hosted-zones`
  return zoneId === undefined ? root : `${root}/${encodeURIComponent(zoneId)}`
}

function recordPath(accountId: string, zoneId: string, recordType: Route53RecordType): string {
  return `${zonePath(accountId, zoneId)}/record-sets/${recordType}`
}

function recordParams(record: Pick<Route53RecordSet['recordSet']['key'], 'owner' | 'routing'>): Record<string, string> {
  return {
    owner: record.owner,
    ...(record.routing.type === 'route53' ? { setIdentifier: record.routing.set_identifier } : {}),
  }
}

export const route53DnsApi = {
  listZones: async (accountId: string, cursor?: string): Promise<ApiResponse<CursorPage<Route53Zone>>> => {
    const { data } = await apiClient.get(centerPath(zonePath(accountId)), { ...centerRequest, params: cursor ? { cursor } : undefined })
    return data
  },
  listRecords: async (accountId: string, zoneId: string, cursor?: string): Promise<ApiResponse<CursorPage<Route53RecordSet> & { zone: Route53Zone }>> => {
    const { data } = await apiClient.get(centerPath(`${zonePath(accountId, zoneId)}/record-sets`), { ...centerRequest, params: cursor ? { cursor } : undefined })
    return data
  },
  getRecord: async (accountId: string, zoneId: string, key: Route53RecordSet['recordSet']['key']): Promise<ApiResponse<Route53RecordSet>> => {
    const { data } = await apiClient.get(centerPath(recordPath(accountId, zoneId, key.recordType)), { ...centerRequest, params: recordParams(key) })
    return data
  },
  putRecord: async (accountId: string, zoneId: string, key: Route53RecordSet['recordSet']['key'], desired: Route53RecordDesired, revision?: string): Promise<ApiResponse<Route53ChangeReceipt>> => {
    const { data } = await apiClient.put(centerPath(recordPath(accountId, zoneId, key.recordType)), {
      guard: revision ? { type: 'match_revision', revision } : { type: 'must_not_exist' },
      desired,
    }, { ...centerRequest, params: recordParams(key) })
    return data
  },
  deleteRecord: async (accountId: string, zoneId: string, record: Route53RecordSet): Promise<ApiResponse<Route53ChangeReceipt>> => {
    const { data } = await apiClient.delete(centerPath(recordPath(accountId, zoneId, record.recordSet.key.recordType)), {
      ...centerRequest,
      params: recordParams(record.recordSet.key),
      data: { expectedRevision: record.revision },
    })
    return data
  },
  observeChange: async (accountId: string, zoneId: string, receipt: string): Promise<ApiResponse<Route53ChangeReceipt>> => {
    const { data } = await apiClient.get(centerPath(`${zonePath(accountId, zoneId)}/changes/${encodeURIComponent(receipt)}`), centerRequest)
    return data
  },
  createZone: async (accountId: string, apex: string, idempotencyKey: string): Promise<ApiResponse<Route53ZoneLifecycleMutation>> => {
    const { data } = await apiClient.post(centerPath(zonePath(accountId)), { apex, idempotencyKey }, centerRequest)
    return data
  },
  observeZoneLifecycle: async (accountId: string, zone: Route53Zone): Promise<ApiResponse<Route53ZoneLifecycleObservation>> => {
    const { data } = await apiClient.get(centerPath(`${zonePath(accountId, zone.zoneId)}/lifecycle`), { ...centerRequest, params: { apex: zone.apex } })
    return data
  },
  deleteZone: async (accountId: string, observation: Route53ZoneLifecycleObservation): Promise<ApiResponse<Route53ZoneLifecycleMutation>> => {
    const { data } = await apiClient.delete(centerPath(`${zonePath(accountId, observation.zone.zoneId)}/lifecycle`), {
      ...centerRequest,
      data: { apex: observation.zone.apex, revision: observation.revision },
    })
    return data
  },
}

export function route53MutationResult(error: unknown): 'conflicted' | 'ambiguous' | 'rejected' {
  const response = (error as { response?: { status?: number; data?: { error?: string } } }).response
  if (response?.data?.error === 'unknown_outcome') return 'ambiguous'
  if (response?.status === 409 || response?.status === 412) return 'conflicted'
  return 'rejected'
}
