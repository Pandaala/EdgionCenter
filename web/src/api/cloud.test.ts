import { afterEach, describe, expect, it, vi } from 'vitest'
import { apiClient } from './client'
import { cloudApi } from './cloud'
import { cloudflareDnsApi } from './cloudflareDns'
import { route53DnsApi } from './route53Dns'

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

})
