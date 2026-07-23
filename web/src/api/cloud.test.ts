import { afterEach, describe, expect, it, vi } from 'vitest'
import { apiClient } from './client'
import { cloudApi } from './cloud'
import { cloudflareDnsApi } from './cloudflareDns'
import { cloudflareWafApi, type CloudflareWafRule, type CloudflareWafRuleset } from './cloudflareWaf'
import { route53DnsApi } from './route53Dns'
import { cloudfrontApi } from './cloudfront'
import { awsWafApi, awsWafMutationResult, isValidAwsRegion, type AwsWafWebAclSummary } from './awsWaf'

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

  it('uses Center-only Route 53 paths and sends both RRset and lifecycle guards', async () => {
    const put = vi.spyOn(apiClient, 'put').mockResolvedValue({ data: { success: true } } as never)
    const remove = vi.spyOn(apiClient, 'delete').mockResolvedValue({ data: { success: true } } as never)
    const key = { owner: 'www.example.com.', recordType: 'A' as const, routing: { type: 'route53' as const, set_identifier: 'blue' } }
    await route53DnsApi.putRecord('aws-main', 'Z0123456789ABCDEF', key, { ttl: { type: 'seconds', seconds: 60 }, values: [{ type: 'A', address: '192.0.2.1' }] }, 'r1')
    await route53DnsApi.deleteZone('aws-main', { zone: { providerAccountId: 'aws-main', zoneId: 'Z0123456789ABCDEF', apex: 'example.com.', visibility: 'public' }, revision: 'z1', authoritativeNameservers: [], delegation: { state: 'not_checked', expectedNameservers: [], parentNameservers: [] }, readiness: 'ready', dnssec: { state: 'disabled', dsRecords: [], externalAction: 'none' }, nonDefaultRecordCount: 0 })
    expect(put).toHaveBeenCalledWith(
      '/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z0123456789ABCDEF/record-sets/A',
      expect.objectContaining({ guard: { type: 'match_revision', revision: 'r1' } }),
      expect.objectContaining({ _skipControllerProxy: true, params: { owner: 'www.example.com.', setIdentifier: 'blue' } }),
    )
    expect(remove).toHaveBeenCalledWith(
      '/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z0123456789ABCDEF/lifecycle',
      expect.objectContaining({ _skipControllerProxy: true, data: { apex: 'example.com.', revision: 'z1' } }),
    )
  })

  it('uses Center-only CloudFront lifecycle paths without a Controller proxy', async () => {
    const post = vi.spyOn(apiClient, 'post').mockResolvedValue({ data: { success: true } } as never)
    const put = vi.spyOn(apiClient, 'put').mockResolvedValue({ data: { success: true } } as never)
    const remove = vi.spyOn(apiClient, 'delete').mockResolvedValue({ data: { success: true } } as never)
    await cloudfrontApi.create('aws-main', { callerReference: 'release-1', originDomainName: 'origin.example.com', originHttpsPort: 443 })
    await cloudfrontApi.updateOrigin('aws-main', 'E2ABCDE12345', { originDomainName: 'replacement.example.com', originHttpsPort: 8443 })
    await cloudfrontApi.delete('aws-main', 'E2ABCDE12345')
    expect(post).toHaveBeenCalledWith('/api/v1/center/aws/cloudfront/accounts/aws-main/distributions', expect.objectContaining({ callerReference: 'release-1' }), expect.objectContaining({ _skipControllerProxy: true }))
    expect(put).toHaveBeenCalledWith('/api/v1/center/aws/cloudfront/accounts/aws-main/distributions/E2ABCDE12345/origin', { originDomainName: 'replacement.example.com', originHttpsPort: 8443 }, expect.objectContaining({ _skipControllerProxy: true }))
    expect(remove).toHaveBeenCalledWith('/api/v1/center/aws/cloudfront/accounts/aws-main/distributions/E2ABCDE12345', expect.objectContaining({ _skipControllerProxy: true, data: { confirmation: 'E2ABCDE12345' } }))
  })

  it('uses scope-separated Center-only AWS WAF paths and resource-exact security confirmation', async () => {
    const put = vi.spyOn(apiClient, 'put').mockResolvedValue({ data: { success: true } } as never)
    const remove = vi.spyOn(apiClient, 'delete').mockResolvedValue({ data: { success: true } } as never)
    const scope = { type: 'regional' as const, region: 'us-east-1' }
    await awsWafApi.updateWebAcl('aws-main', scope, 'acl-1', { lockToken: 'opaque-lock', visibility: { metricName: 'edge', cloudwatchMetricsEnabled: true, sampledRequestsEnabled: true } })
    await awsWafApi.deleteRule('aws-main', scope, 'acl-1', 'rule-1', { lockToken: 'opaque-lock', confirmation: 'acl-1/rule-1' })
    await awsWafApi.deleteIpSet('aws-main', scope, 'ip-set-1', { lockToken: 'opaque-lock', confirmation: 'ip-set-1' })
    expect(put).toHaveBeenCalledWith(
      '/api/v1/center/aws/waf/accounts/aws-main/scopes/regional/web-acls/acl-1',
      expect.objectContaining({ lockToken: 'opaque-lock', visibility: expect.objectContaining({ metricName: 'edge' }) }),
      expect.objectContaining({ _skipControllerProxy: true, params: { region: 'us-east-1' } }),
    )
    expect(remove).toHaveBeenNthCalledWith(1,
      '/api/v1/center/aws/waf/accounts/aws-main/scopes/regional/web-acls/acl-1/rules/rule-1/security-weaken',
      expect.objectContaining({ _skipControllerProxy: true, params: { region: 'us-east-1' }, data: { lockToken: 'opaque-lock', confirmation: 'acl-1/rule-1' } }),
    )
    expect(remove).toHaveBeenNthCalledWith(2,
      '/api/v1/center/aws/waf/accounts/aws-main/scopes/regional/ip-sets/ip-set-1/security-weaken',
      expect.objectContaining({ _skipControllerProxy: true, params: { region: 'us-east-1' }, data: { lockToken: 'opaque-lock', confirmation: 'ip-set-1' } }),
    )
  })

  it('sends managed WAF patches without empty override arrays and routes Count through security-weaken', async () => {
    const put = vi.spyOn(apiClient, 'put').mockResolvedValue({ data: { success: true } } as never)
    const scope = { type: 'cloudfront' as const }
    await awsWafApi.managedException('aws-main', scope, 'acl-1', 'managed-1', {
      lockToken: 'opaque-lock', excludedRules: ['NoUserAgent_HEADER'], confirmation: 'acl-1/managed-1',
    })
    await awsWafApi.securityWeakenRule('aws-main', scope, 'acl-1', 'managed-1', {
      lockToken: 'opaque-lock', managedOverrideAction: 'count', confirmation: 'acl-1/managed-1',
    })
    expect(put).toHaveBeenNthCalledWith(1,
      '/api/v1/center/aws/waf/accounts/aws-main/scopes/cloudfront/web-acls/acl-1/rules/managed-1/exceptions',
      { lockToken: 'opaque-lock', excludedRules: ['NoUserAgent_HEADER'], confirmation: 'acl-1/managed-1' },
      expect.objectContaining({ _skipControllerProxy: true }),
    )
    expect(put).toHaveBeenNthCalledWith(2,
      '/api/v1/center/aws/waf/accounts/aws-main/scopes/cloudfront/web-acls/acl-1/rules/managed-1/security-weaken',
      { lockToken: 'opaque-lock', managedOverrideAction: 'count', confirmation: 'acl-1/managed-1' },
      expect.objectContaining({ _skipControllerProxy: true }),
    )
  })

  it('shares the tagged WAF scope fixture and turns an unknown provider outcome into a refresh lock', () => {
    const fixture: AwsWafWebAclSummary = {
      id: 'acl-1', name: 'edge', arn: 'arn:aws:wafv2:us-east-1:123:regional/webacl/edge',
      scope: { type: 'regional', region: 'us-east-1' }, capacity: 20, lockTokenPresent: true,
    }
    expect(fixture.scope).toEqual({ type: 'regional', region: 'us-east-1' })
    expect(isValidAwsRegion('us-gov-west-1')).toBe(true)
    expect(isValidAwsRegion('not a region')).toBe(false)
    expect(awsWafMutationResult({ response: { status: 409, data: { error: 'unknown_outcome' } } })).toBe('ambiguous')
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
