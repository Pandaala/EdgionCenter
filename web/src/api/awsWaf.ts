import { apiClient } from './client'
import type { ApiResponse } from './types'

const request = { _skipControllerProxy: true } as any
const root = '/api/v1/center/aws/waf/accounts'

export type AwsWafScope = { type: 'cloudfront' } | { type: 'regional'; region: string }
export type AwsWafRegionalScope = Extract<AwsWafScope, { type: 'regional' }>
export type AwsWafAction = 'allow' | 'block' | 'count' | 'challenge' | 'captcha'
export type AwsWafDefaultAction = 'allow' | 'block'
export type AwsWafManagedRuleOverrideAction = 'none' | 'count'
export type AwsWafAddressVersion = 'ipv4' | 'ipv6'
export type AwsWafStatement =
  | { kind: 'managed_rule_group'; vendorName: string; name: string; version?: string; excludedRules: string[]; ruleActionOverrides: Array<{ name: string; action: AwsWafAction }> }
  | { kind: 'ip_set_reference'; arn: string }
  | { kind: 'rate_based'; limit: number; scopeDownIpSet?: { arn: string } }

export interface AwsWafVisibility { cloudwatchMetricsEnabled: boolean; sampledRequestsEnabled: boolean; metricName: string }
export interface AwsWafRule { name: string; priority: number; action?: AwsWafAction; managedOverrideAction?: AwsWafManagedRuleOverrideAction; statement: AwsWafStatement; visibility: AwsWafVisibility; ownership: 'center_owned' | 'external'; reference?: string }
export interface AwsWafWebAcl { id: string; name: string; arn: string; scope: AwsWafScope; defaultAction: AwsWafDefaultAction; visibility: AwsWafVisibility; capacity: number; lockToken: string; rules: AwsWafRule[] }
export interface AwsWafWebAclSummary { id: string; name: string; arn: string; scope: AwsWafScope; capacity: number; lockTokenPresent: boolean }
export interface AwsWafIpSet { id: string; name: string; arn: string; scope: AwsWafScope; addressVersion: AwsWafAddressVersion; addresses: string[]; lockToken: string }
export interface AwsWafCatalog { vendorName: string; name: string; versions: string[] }
export interface AwsWafCapacity { requiredWcu: number; allowed: boolean; reason: string }
export interface AwsWafAssociation { resourceArn: string; resourceKind: 'application_load_balancer' | 'api_gateway_stage' | 'app_sync_api' | 'cognito_user_pool'; webAclId: string; targetDeploymentAuthority: string }
export interface AwsWafRuleWrite { reference: string; lockToken: string; name: string; priority: number; action?: AwsWafAction; managedOverrideAction?: AwsWafManagedRuleOverrideAction; statement: AwsWafStatement; visibility: AwsWafVisibility }
export interface AwsWafManagedExceptionWrite { lockToken: string; excludedRules?: string[]; ruleActionOverrides?: Array<{ name: string; action: AwsWafAction }>; confirmation: string }
export type AwsWafRuleSecurityWeakenWrite =
  | { lockToken: string; action: 'allow' | 'count'; managedOverrideAction?: never; confirmation: string }
  | { lockToken: string; action?: never; managedOverrideAction: 'count'; confirmation: string }

function scopePath(accountId: string, scope: AwsWafScope): string {
  return `${root}/${encodeURIComponent(accountId)}/scopes/${scope.type}`
}
export function isValidAwsRegion(region: string): boolean {
  return /^(?:[a-z]{2})(?:-[a-z]+)*-[a-z]+-\d+$/.test(region) && region.length <= 32
}
function path(accountId: string, scope: AwsWafScope, suffix: string): string { return `${scopePath(accountId, scope)}/${suffix}` }
function requestConfig(scope: AwsWafScope): any { return scope.type === 'regional' ? { ...request, params: { region: scope.region } } : request }

export const awsWafApi = {
  listWebAcls: async (account: string, scope: AwsWafScope): Promise<ApiResponse<AwsWafWebAclSummary[]>> => (await apiClient.get(path(account, scope, 'web-acls').split('?')[0], requestConfig(scope))).data,
  getWebAcl: async (account: string, scope: AwsWafScope, id: string): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.get(path(account, scope, `web-acls/${encodeURIComponent(id)}`).split('?')[0], requestConfig(scope))).data,
  listIpSets: async (account: string, scope: AwsWafScope): Promise<ApiResponse<AwsWafIpSet[]>> => (await apiClient.get(path(account, scope, 'ip-sets').split('?')[0], requestConfig(scope))).data,
  listCatalog: async (account: string, scope: AwsWafScope): Promise<ApiResponse<AwsWafCatalog[]>> => (await apiClient.get(path(account, scope, 'managed-rule-groups').split('?')[0], requestConfig(scope))).data,
  createWebAcl: async (account: string, scope: AwsWafScope, requestBody: { name: string; defaultAction: AwsWafDefaultAction; visibility: AwsWafVisibility }): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.post(path(account, scope, 'web-acls').split('?')[0], requestBody, requestConfig(scope))).data,
  updateWebAcl: async (account: string, scope: AwsWafScope, id: string, requestBody: { lockToken: string; visibility: AwsWafVisibility }): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.put(path(account, scope, `web-acls/${encodeURIComponent(id)}`).split('?')[0], requestBody, requestConfig(scope))).data,
  weakenWebAcl: async (account: string, scope: AwsWafScope, id: string, requestBody: { lockToken: string; defaultAction: AwsWafDefaultAction; confirmation: string }): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.put(path(account, scope, `web-acls/${encodeURIComponent(id)}/security-weaken`).split('?')[0], requestBody, requestConfig(scope))).data,
  deleteWebAcl: async (account: string, scope: AwsWafScope, id: string, requestBody: { lockToken: string; confirmation: string }): Promise<void> => { await apiClient.delete(path(account, scope, `web-acls/${encodeURIComponent(id)}/security-weaken`).split('?')[0], { ...requestConfig(scope), data: requestBody }) },
  createRule: async (account: string, scope: AwsWafScope, acl: string, requestBody: AwsWafRuleWrite): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.post(path(account, scope, `web-acls/${encodeURIComponent(acl)}/rules`).split('?')[0], requestBody, requestConfig(scope))).data,
  updateRule: async (account: string, scope: AwsWafScope, acl: string, reference: string, requestBody: AwsWafRuleWrite): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.put(path(account, scope, `web-acls/${encodeURIComponent(acl)}/rules/${encodeURIComponent(reference)}`).split('?')[0], requestBody, requestConfig(scope))).data,
  deleteRule: async (account: string, scope: AwsWafScope, acl: string, reference: string, requestBody: { lockToken: string; confirmation: string }): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.delete(path(account, scope, `web-acls/${encodeURIComponent(acl)}/rules/${encodeURIComponent(reference)}/security-weaken`).split('?')[0], { ...requestConfig(scope), data: requestBody })).data,
  managedException: async (account: string, scope: AwsWafScope, acl: string, reference: string, requestBody: AwsWafManagedExceptionWrite): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.put(path(account, scope, `web-acls/${encodeURIComponent(acl)}/rules/${encodeURIComponent(reference)}/exceptions`).split('?')[0], requestBody, requestConfig(scope))).data,
  capacity: async (account: string, scope: AwsWafScope, rules: AwsWafRuleWrite[]): Promise<ApiResponse<AwsWafCapacity>> => (await apiClient.post(path(account, scope, 'capacity').split('?')[0], { rules }, requestConfig(scope))).data,
  associations: async (account: string, scope: AwsWafScope, id: string): Promise<ApiResponse<AwsWafAssociation[]>> => (await apiClient.get(path(account, scope, `web-acls/${encodeURIComponent(id)}/associations`).split('?')[0], requestConfig(scope))).data,
  attachRegional: async (account: string, scope: AwsWafRegionalScope, id: string, requestBody: { resourceArn: string; resourceKind: AwsWafAssociation['resourceKind'] }): Promise<ApiResponse<AwsWafAssociation[]>> => (await apiClient.post(path(account, scope, `web-acls/${encodeURIComponent(id)}/associations`).split('?')[0], requestBody, requestConfig(scope))).data,
  detachRegional: async (account: string, scope: AwsWafRegionalScope, requestBody: { resourceArn: string; resourceKind: AwsWafAssociation['resourceKind']; confirmation: string }): Promise<void> => { await apiClient.delete(path(account, scope, 'associations').split('?')[0], { ...requestConfig(scope), data: requestBody }) },
  createIpSet: async (account: string, scope: AwsWafScope, requestBody: { name: string; addressVersion: AwsWafAddressVersion; addresses: string[] }): Promise<ApiResponse<AwsWafIpSet>> => (await apiClient.post(path(account, scope, 'ip-sets').split('?')[0], requestBody, requestConfig(scope))).data,
  updateIpSet: async (account: string, scope: AwsWafScope, id: string, requestBody: { lockToken: string; addresses: string[] }): Promise<ApiResponse<AwsWafIpSet>> => (await apiClient.put(path(account, scope, `ip-sets/${encodeURIComponent(id)}`).split('?')[0], requestBody, requestConfig(scope))).data,
  deleteIpSet: async (account: string, scope: AwsWafScope, id: string, requestBody: { lockToken: string; confirmation: string }): Promise<void> => { await apiClient.delete(path(account, scope, `ip-sets/${encodeURIComponent(id)}/security-weaken`).split('?')[0], { ...requestConfig(scope), data: requestBody }) },
  securityWeakenRule: async (account: string, scope: AwsWafScope, acl: string, reference: string, requestBody: AwsWafRuleSecurityWeakenWrite): Promise<ApiResponse<AwsWafWebAcl>> => (await apiClient.put(path(account, scope, `web-acls/${encodeURIComponent(acl)}/rules/${encodeURIComponent(reference)}/security-weaken`).split('?')[0], requestBody, requestConfig(scope))).data,
}

export function awsWafMutationResult(error: unknown): 'conflicted' | 'ambiguous' | 'rejected' {
  const response = (error as { response?: { status?: number; data?: { error?: string } } }).response
  if (response?.data?.error === 'unknown_outcome') return 'ambiguous'
  if (response?.status === 409 || response?.status === 412) return 'conflicted'
  return 'rejected'
}
