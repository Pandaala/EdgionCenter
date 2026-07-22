import { apiClient } from './client'
import type { ApiResponse, ListResponse } from './types'

/** Center cloud APIs are never controller-proxied, even while a controller view is open. */
const centerRequest = { _skipControllerProxy: true } as any
const centerPath = (path: string) => `/api/v1/center/${path}`

export type CloudProvider = 'cloudflare' | 'aws' | 'google_cloud'
export type ManagementPolicy = 'managed' | 'observe_only'

export type ProviderAccountScope =
  | { provider: 'cloudflare'; accountId: string }
  | { provider: 'aws'; accountId: string }
  | { provider: 'google_cloud'; projectId: string }

/**
 * References select credentials owned outside the dashboard. These types
 * deliberately contain no token, password, private key, or secret payload.
 */
export type CredentialSource =
  | { type: 'static_secret'; credentialRef: string }
  | { type: 'ambient' }
  | { type: 'federated'; subjectTokenRef?: string; targetPrincipal: string; audience?: string }
  | { type: 'assume_identity'; baseCredentialRef?: string; targetPrincipal: string; externalIdRef?: string }

export interface ProviderAccountDesired {
  displayName: string
  owner?: string
  labels: Record<string, string>
  managementPolicy: ManagementPolicy
  provider: CloudProvider
  scope: ProviderAccountScope
  credentialSource: CredentialSource
}

export interface ProviderAccount extends ProviderAccountDesired {
  accountId: string
  generation: number
  deletionPolicy: 'retain'
}

export interface CredentialInspection {
  providerAccountId: string
  providerAccountGeneration: number
  state: 'valid' | 'invalid' | 'unknown' | string
  identity?: { provider: CloudProvider; scope?: string }
  expiresAtUnixMs?: number
  issues: Array<{ kind: string }>
}

export type CapabilityState = 'affirmative' | 'negative' | 'unknown' | 'not_applicable'

export interface CapabilityDimensionObservation {
  dimension: string
  action?: string
  state: CapabilityState
  reason?: string
  evidence: string
  observedAtUnixMs: number
  validUntilUnixMs: number
}

export interface ProviderCapabilitySnapshot {
  observedAccountGeneration: number
  accountGenerationMatches: boolean
  authorityState: 'unknown' | 'stale'
  credentialAuthorityState: 'unknown'
  state: 'complete' | 'partial' | 'failed'
  discoveredAtUnixMs: number
  observations: Array<{
    capability: { family: 'dns' | 'waf'; name: string }
    dimensions: CapabilityDimensionObservation[]
  }>
  issues: Array<{ severity: 'warning' | 'blocking'; scope: unknown; reason: string }>
}

export interface ProviderCapabilityRead {
  providerAccountId: string
  provider: CloudProvider
  currentProviderAccountGeneration: number
  scope: { type: 'account' }
  snapshotState: 'not_discovered' | 'observed'
  snapshot?: ProviderCapabilitySnapshot
}

export interface ProviderAccountResponse<T> {
  body: ApiResponse<T>
  etag?: string
}

export const cloudApi = {
  listAccounts: async (): Promise<ListResponse<ProviderAccount>> => {
    const { data } = await apiClient.get(centerPath('cloud/provider-accounts'), centerRequest)
    return data
  },
  getAccount: async (accountId: string): Promise<ProviderAccountResponse<ProviderAccount>> => {
    const response = await apiClient.get(centerPath(`cloud/provider-accounts/${encodeURIComponent(accountId)}`), centerRequest)
    return { body: response.data, etag: response.headers.etag }
  },
  createAccount: async (accountId: string, desired: ProviderAccountDesired): Promise<ProviderAccountResponse<ProviderAccount>> => {
    const response = await apiClient.post(centerPath('cloud/provider-accounts'), { accountId, desired }, centerRequest)
    return { body: response.data, etag: response.headers.etag }
  },
  replaceAccount: async (accountId: string, desired: ProviderAccountDesired, etag: string): Promise<ProviderAccountResponse<ProviderAccount>> => {
    const response = await apiClient.put(centerPath(`cloud/provider-accounts/${encodeURIComponent(accountId)}`), { desired }, {
      ...centerRequest,
      headers: { 'If-Match': etag },
    })
    return { body: response.data, etag: response.headers.etag }
  },
  getCapabilities: async (accountId: string): Promise<ApiResponse<ProviderCapabilityRead>> => {
    const { data } = await apiClient.get(centerPath(`cloud/provider-capabilities/accounts/${encodeURIComponent(accountId)}`), {
      ...centerRequest,
      params: { scope: 'account' },
    })
    return data
  },
  inspectCredentials: async (accountId: string): Promise<ApiResponse<CredentialInspection>> => {
    const { data } = await apiClient.post(centerPath(`cloud/provider-credential-inspections/accounts/${encodeURIComponent(accountId)}/refresh`), undefined, centerRequest)
    return data
  },
}
