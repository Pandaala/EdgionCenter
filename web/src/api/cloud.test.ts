import { afterEach, describe, expect, it, vi } from 'vitest'
import { apiClient } from './client'
import { cloudApi } from './cloud'
import { cloudflareDnsApi } from './cloudflareDns'
import { cloudflareWafApi, type CloudflareWafRule, type CloudflareWafRuleset } from './cloudflareWaf'

afterEach(() => { vi.restoreAllMocks() })

describe('Center cloud API routing', () => {
  it('uses explicit Center paths and never enables the controller proxy', async () => {
    const get = vi.spyOn(apiClient, 'get').mockResolvedValue({ data: { success: true, data: [] } } as never)
    await cloudApi.listAccounts()
    await cloudflareDnsApi.listZones('cf-main', 'opaque-cursor')

    expect(get).toHaveBeenNthCalledWith(1, '/api/v1/center/cloud/provider-accounts', expect.objectContaining({ _skipControllerProxy: true }))
    expect(get).toHaveBeenNthCalledWith(2, '/api/v1/center/cloudflare/dns/accounts/cf-main/zones', expect.objectContaining({ _skipControllerProxy: true, params: { cursor: 'opaque-cursor' } }))
  })

  it('sends a revision guard for direct Cloudflare DNS record writes', async () => {
    const put = vi.spyOn(apiClient, 'put').mockResolvedValue({ data: { success: true } } as never)
    await cloudflareDnsApi.putRecord('cf-main', '0123456789abcdef0123456789abcdef', {
      providerAccountId: 'cf-main', zoneId: '0123456789abcdef0123456789abcdef', zoneApex: 'example.com.', zoneVisibility: 'public', owner: 'www.example.com.', recordType: 'A', ttl: { type: 'automatic' }, values: [{ type: 'A', address: '192.0.2.1' }], proxy: 'dns_only', cnameFlattening: 'provider_default', tags: [], control: { type: 'manual' }, providerObjectIds: [], revision: 'revision-1',
    }, {
      guard: { type: 'match_revision', revision: 'revision-1' }, ttl: { type: 'automatic' }, values: [{ type: 'A', address: '192.0.2.1' }], proxy: 'dns_only', cnameFlattening: 'provider_default', tags: [],
    })
    expect(put).toHaveBeenCalledWith(
      '/api/v1/center/cloudflare/dns/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/record-sets/A',
      expect.objectContaining({ guard: { type: 'match_revision', revision: 'revision-1' } }),
      expect.objectContaining({ _skipControllerProxy: true, params: { owner: 'www.example.com.' } }),
    )
  })

  it('uses the Center-only WAF security-weaken route and disables a managed execute wrapper without inventing an override id', async () => {
    const put = vi.spyOn(apiClient, 'put').mockResolvedValue({ data: { success: true } } as never)
    const ruleset: CloudflareWafRuleset = { providerAccountId: 'cf-main', zoneId: '0123456789abcdef0123456789abcdef', phase: 'managed', availability: 'available', rulesetId: 'entry-point', version: 'v1', rules: [] }
    const rule: CloudflareWafRule = { ruleId: 'wrapper-rule', version: 'r1', action: 'execute', enabled: true, ownership: 'center_owned', position: 0, definition: { kind: 'managed', reference: 'managed', description: 'managed rule', expression: 'http.request.uri.path eq "/"', managedRulesetId: 'cf-managed', overrides: [] } }
    await cloudflareWafApi.securityWeaken('cf-main', ruleset.zoneId, ruleset, rule, { reference: 'managed', description: 'managed rule', expression: 'http.request.uri.path eq "/"', action: 'block', managedRulesetId: 'cf-managed' })
    expect(put).toHaveBeenCalledWith(
      '/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/managed-rules/wrapper-rule/security-weaken',
      expect.objectContaining({ enabled: false, overrides: [], confirmation: 'WEAKEN_CLOUDFLARE_WAF' }),
      expect.objectContaining({ _skipControllerProxy: true }),
    )
  })

  it('creates a first bounded WAF rule with an explicit null ruleset guard and never uses a Controller proxy', async () => {
    const post = vi.spyOn(apiClient, 'post').mockResolvedValue({ data: { success: true } } as never)
    const ruleset: CloudflareWafRuleset = { providerAccountId: 'cf-main', zoneId: '0123456789abcdef0123456789abcdef', phase: 'custom', availability: 'entry_point_absent', rules: [] }
    await cloudflareWafApi.create('cf-main', ruleset.zoneId, ruleset, { reference: 'custom-1', description: 'custom rule', expression: 'http.request.uri.path eq "/"', action: 'block' })
    expect(post).toHaveBeenCalledWith(
      '/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/custom-rules',
      expect.objectContaining({ guard: { rulesetId: null, rulesetVersion: null }, action: 'block' }),
      expect.objectContaining({ _skipControllerProxy: true }),
    )
  })

  it('uses the dedicated single-rule WAF ordering route with the current version guard', async () => {
    const put = vi.spyOn(apiClient, 'put').mockResolvedValue({ data: { success: true } } as never)
    const ruleset: CloudflareWafRuleset = { providerAccountId: 'cf-main', zoneId: '0123456789abcdef0123456789abcdef', phase: 'custom', availability: 'available', rulesetId: 'entry-point', version: 'v7', rules: [] }
    const rule: CloudflareWafRule = { ruleId: 'custom-1', version: 'r1', action: 'block', enabled: true, ownership: 'center_owned', position: 0 }
    await cloudflareWafApi.order('cf-main', ruleset.zoneId, ruleset, rule, { type: 'index', index: 2 })
    expect(put).toHaveBeenCalledWith(
      '/api/v1/center/cloudflare/waf/accounts/cf-main/zones/0123456789abcdef0123456789abcdef/custom-rules/custom-1/order',
      { guard: { rulesetId: 'entry-point', rulesetVersion: 'v7' }, position: { type: 'index', index: 2 } },
      expect.objectContaining({ _skipControllerProxy: true }),
    )
  })
})
