import { apiClient } from './client'
import type { ApiResponse } from './types'

/** Cloudflare WAF is a Center-owned bounded API and never uses a Controller proxy. */
const centerRequest = { _skipControllerProxy: true } as any
const path = (accountId: string, zoneId: string, suffix = '') =>
  `/api/v1/center/cloudflare/waf/accounts/${encodeURIComponent(accountId)}/zones/${encodeURIComponent(zoneId)}${suffix}`

export type CloudflareWafPhase = 'managed' | 'custom' | 'rate_limit'
export type CloudflareWafAction = 'block' | 'challenge' | 'managed_challenge' | 'log'
export type CloudflareWafOwnership = 'center_owned' | 'observe_only'
export type CloudflareWafAvailability = 'available' | 'entry_point_absent' | 'permission_denied' | 'quota_limited' | 'unavailable'
export type CloudflareWafPosition = { type: 'first' } | { type: 'before'; ruleId: string } | { type: 'after'; ruleId: string } | { type: 'index'; index: number }
export type CloudflareWafGuard = { rulesetId: string; rulesetVersion: string }
/** Both fields are explicit for the first deployment (`null`, never omitted). */
export type CloudflareWafCreateGuard = { rulesetId: string | null; rulesetVersion: string | null }

export interface CloudflareWafOverride {
  managedRuleId: string
  action?: CloudflareWafAction
  enabled?: boolean
}

export type CloudflareWafDefinition =
  | { kind: 'managed'; reference: string; description: string; expression: string; managedRulesetId: string; overrides: CloudflareWafOverride[] }
  | { kind: 'managed_exception'; reference: string; description: string; expression: string; managedRulesetIds: string[]; position: CloudflareWafPosition }
  | { kind: 'custom'; reference: string; description: string; expression: string; action: CloudflareWafAction }
  | { kind: 'rate_limit'; reference: string; description: string; expression: string; action: CloudflareWafAction; characteristics: Array<'ip_source' | 'colo'>; periodSecs: number; requestsPerPeriod: number; mitigationTimeoutSecs: number }

export interface CloudflareWafRule {
  ruleId: string
  version: string
  action: string
  enabled: boolean
  ownership: CloudflareWafOwnership
  position: number
  definition?: CloudflareWafDefinition
}

export interface CloudflareWafRuleset {
  providerAccountId: string
  zoneId: string
  phase: CloudflareWafPhase
  availability: CloudflareWafAvailability
  rulesetId?: string
  version?: string
  rules: CloudflareWafRule[]
}

export interface CloudflareWafInventory { rulesets: CloudflareWafRuleset[] }
export interface CloudflareWafMutationResult { providerAccountId: string; zoneId: string; phase: CloudflareWafPhase; rulesetId: string; rulesetVersion: string; ruleId: string; securityWeakeningConfirmed: boolean }

export interface WafRuleValues {
  reference: string
  description: string
  expression: string
  action: CloudflareWafAction
  managedRulesetId?: string
  managedRulesetIds?: string[]
  characteristics?: Array<'ip_source' | 'colo'>
  periodSecs?: number
  requestsPerPeriod?: number
  mitigationTimeoutSecs?: number
}

function guard(ruleset: CloudflareWafRuleset): CloudflareWafGuard {
  if (!ruleset.rulesetId || !ruleset.version) throw new Error('missing ruleset version')
  return { rulesetId: ruleset.rulesetId, rulesetVersion: ruleset.version }
}

function createGuard(ruleset: CloudflareWafRuleset): CloudflareWafCreateGuard {
  return ruleset.rulesetId && ruleset.version ? { rulesetId: ruleset.rulesetId, rulesetVersion: ruleset.version } : { rulesetId: null, rulesetVersion: null }
}

function phasePath(phase: CloudflareWafPhase): string {
  return phase === 'rate_limit' ? 'rate-limits' : `${phase}-rules`
}

export const cloudflareWafApi = {
  listRulesets: async (accountId: string, zoneId: string): Promise<ApiResponse<CloudflareWafInventory>> => {
    const { data } = await apiClient.get(path(accountId, zoneId, '/rulesets'), centerRequest)
    return data
  },
  create: async (accountId: string, zoneId: string, ruleset: CloudflareWafRuleset, values: WafRuleValues, position?: CloudflareWafPosition): Promise<ApiResponse<CloudflareWafMutationResult>> => {
    const body = ruleset.phase === 'managed'
      ? { guard: createGuard(ruleset), reference: values.reference, description: values.description, expression: values.expression, managedRulesetId: values.managedRulesetId, overrides: [], position }
      : ruleset.phase === 'custom'
        ? { guard: createGuard(ruleset), reference: values.reference, description: values.description, expression: values.expression, action: values.action, position }
        : { guard: createGuard(ruleset), reference: values.reference, description: values.description, expression: values.expression, action: values.action, characteristics: values.characteristics, requestsPerPeriod: values.requestsPerPeriod, periodSecs: values.periodSecs, mitigationTimeoutSecs: values.mitigationTimeoutSecs, position }
    const { data } = await apiClient.post(path(accountId, zoneId, `/${phasePath(ruleset.phase)}`), body, centerRequest)
    return data
  },
  update: async (accountId: string, zoneId: string, ruleset: CloudflareWafRuleset, rule: CloudflareWafRule, values: WafRuleValues): Promise<ApiResponse<CloudflareWafMutationResult>> => {
    const body = ruleset.phase === 'managed'
      ? { guard: guard(ruleset), reference: values.reference, description: values.description, expression: values.expression, managedRulesetId: values.managedRulesetId, overrides: [] }
      : ruleset.phase === 'custom'
        ? { guard: guard(ruleset), reference: values.reference, description: values.description, expression: values.expression, action: values.action }
        : { guard: guard(ruleset), reference: values.reference, description: values.description, expression: values.expression, action: values.action, characteristics: values.characteristics, requestsPerPeriod: values.requestsPerPeriod, periodSecs: values.periodSecs, mitigationTimeoutSecs: values.mitigationTimeoutSecs }
    const { data } = await apiClient.put(path(accountId, zoneId, `/${phasePath(ruleset.phase)}/${encodeURIComponent(rule.ruleId)}`), body, centerRequest)
    return data
  },
  order: async (accountId: string, zoneId: string, ruleset: CloudflareWafRuleset, rule: CloudflareWafRule, position: CloudflareWafPosition): Promise<ApiResponse<CloudflareWafMutationResult>> => {
    const { data } = await apiClient.put(path(accountId, zoneId, `/${phasePath(ruleset.phase)}/${encodeURIComponent(rule.ruleId)}/order`), { guard: guard(ruleset), position }, centerRequest)
    return data
  },
  securityWeaken: async (accountId: string, zoneId: string, ruleset: CloudflareWafRuleset, rule: CloudflareWafRule, values: WafRuleValues): Promise<ApiResponse<CloudflareWafMutationResult>> => {
    const body = ruleset.phase === 'managed'
      ? { guard: guard(ruleset), reference: values.reference, description: values.description, expression: values.expression, managedRulesetId: values.managedRulesetId, overrides: [], enabled: false, confirmation: 'WEAKEN_CLOUDFLARE_WAF' }
      : ruleset.phase === 'custom'
        ? { guard: guard(ruleset), reference: values.reference, description: values.description, expression: values.expression, action: 'log' as const, enabled: true, confirmation: 'WEAKEN_CLOUDFLARE_WAF' }
        : { guard: guard(ruleset), reference: values.reference, description: values.description, expression: values.expression, action: 'log' as const, characteristics: values.characteristics, requestsPerPeriod: values.requestsPerPeriod, periodSecs: values.periodSecs, mitigationTimeoutSecs: values.mitigationTimeoutSecs, enabled: true, confirmation: 'WEAKEN_CLOUDFLARE_WAF' }
    const { data } = await apiClient.put(path(accountId, zoneId, `/${phasePath(ruleset.phase)}/${encodeURIComponent(rule.ruleId)}/security-weaken`), body, centerRequest)
    return data
  },
  delete: async (accountId: string, zoneId: string, ruleset: CloudflareWafRuleset, rule: CloudflareWafRule): Promise<ApiResponse<CloudflareWafMutationResult>> => {
    const { data } = await apiClient.delete(path(accountId, zoneId, `/${phasePath(ruleset.phase)}/${encodeURIComponent(rule.ruleId)}/security-weaken`), { ...centerRequest, data: { guard: guard(ruleset), confirmation: 'WEAKEN_CLOUDFLARE_WAF' } })
    return data
  },
  setManagedException: async (accountId: string, zoneId: string, ruleset: CloudflareWafRuleset, values: WafRuleValues, position: CloudflareWafPosition): Promise<ApiResponse<CloudflareWafMutationResult>> => {
    const { data } = await apiClient.put(path(accountId, zoneId, '/managed-rules/exceptions'), { guard: guard(ruleset), reference: values.reference, description: values.description, expression: values.expression, managedRulesetIds: values.managedRulesetIds, position, confirmation: 'WEAKEN_CLOUDFLARE_WAF' }, centerRequest)
    return data
  },
}
