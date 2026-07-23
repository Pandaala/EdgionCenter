import { apiClient } from './client'
import type { ApiResponse } from './types'

/** CloudFront is a Center-owned AWS surface and must never use a Controller proxy. */
const centerRequest = { _skipControllerProxy: true } as any
const centerPath = (path: string) => `/api/v1/center/aws/cloudfront/${path}`

export interface CloudFrontDistribution {
  id: string
  arn: string
  domainName: string
  status: string
  enabled: boolean
  etag: string
  deployed: boolean
  webAclId?: string
  supportedOrigin?: { domainName: string; httpsPort: number }
}

function distributionPath(accountId: string, distributionId?: string): string {
  const root = `accounts/${encodeURIComponent(accountId)}/distributions`
  return distributionId === undefined ? root : `${root}/${encodeURIComponent(distributionId)}`
}

export const cloudfrontApi = {
  list: async (accountId: string): Promise<ApiResponse<CloudFrontDistribution[]>> => {
    const { data } = await apiClient.get(centerPath(distributionPath(accountId)), centerRequest)
    return data
  },
  get: async (accountId: string, distributionId: string): Promise<ApiResponse<CloudFrontDistribution>> => {
    const { data } = await apiClient.get(centerPath(distributionPath(accountId, distributionId)), centerRequest)
    return data
  },
  observe: async (accountId: string, distributionId: string): Promise<ApiResponse<CloudFrontDistribution>> => {
    const { data } = await apiClient.get(centerPath(`${distributionPath(accountId, distributionId)}/observation`), centerRequest)
    return data
  },
  create: async (accountId: string, request: { callerReference: string; originDomainName: string; originHttpsPort: number }): Promise<ApiResponse<CloudFrontDistribution>> => {
    const { data } = await apiClient.post(centerPath(distributionPath(accountId)), request, centerRequest)
    return data
  },
  updateOrigin: async (accountId: string, distributionId: string, request: { originDomainName: string; originHttpsPort: number }): Promise<ApiResponse<CloudFrontDistribution>> => {
    const { data } = await apiClient.put(centerPath(`${distributionPath(accountId, distributionId)}/origin`), request, centerRequest)
    return data
  },
  enable: async (accountId: string, distributionId: string): Promise<ApiResponse<CloudFrontDistribution>> => {
    const { data } = await apiClient.post(centerPath(`${distributionPath(accountId, distributionId)}/enable`), undefined, centerRequest)
    return data
  },
  disable: async (accountId: string, distributionId: string): Promise<ApiResponse<CloudFrontDistribution>> => {
    const { data } = await apiClient.post(centerPath(`${distributionPath(accountId, distributionId)}/disable`), undefined, centerRequest)
    return data
  },
  delete: async (accountId: string, distributionId: string): Promise<void> => {
    await apiClient.delete(centerPath(distributionPath(accountId, distributionId)), { ...centerRequest, data: { confirmation: distributionId } })
  },
  /** CLD-29A association boundary: changes only the Distribution WebACLId. */
  setWebAcl: async (accountId: string, distributionId: string, request: { webAclId: string }): Promise<ApiResponse<CloudFrontDistribution>> => {
    const { data } = await apiClient.put(centerPath(`${distributionPath(accountId, distributionId)}/web-acl`), request, centerRequest)
    return data
  },
  detachWebAcl: async (accountId: string, distributionId: string): Promise<ApiResponse<CloudFrontDistribution>> => {
    const { data } = await apiClient.delete(centerPath(`${distributionPath(accountId, distributionId)}/web-acl`), { ...centerRequest, data: { confirmation: distributionId } })
    return data
  },
}

export function cloudfrontMutationResult(error: unknown): 'conflicted' | 'ambiguous' | 'rejected' {
  const response = (error as { response?: { status?: number; data?: { error?: string } } }).response
  if (response?.data?.error === 'unknown_outcome') return 'ambiguous'
  if (response?.status === 409 || response?.status === 412) return 'conflicted'
  return 'rejected'
}
