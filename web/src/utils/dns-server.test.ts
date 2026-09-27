import { describe, expect, it } from 'vitest'
import type { HttpDnsConfig, WebhookConfig } from '@/types/link-sys'
import { isValidDnsServer } from './dns-server'
import { createEmpty, validateLinkSys } from './linksys'

describe('DNS server literals', () => {
  it('matches literal IPv4/IPv6 and socket forms without accepting URL normalization', () => {
    for (const value of ['1.1.1.1', '1.1.1.1:53', '1.1.1.1:0', '::1', '2001:db8::1', '[2001:db8::1]', '[::1]:5353']) expect(isValidDnsServer(value), value).toBe(true)
    for (const value of ['dns.example.com', '127.1', '01.1.1.1', '999.1.1.1', '1.1.1.1:65536', '[::1]:', '[::1]:-1', '[::1]/', '1.1.1.1 ', 'https://1.1.1.1']) expect(isValidDnsServer(value), value).toBe(false)
  })

  it('rejects unusable HTTP DNS fallback while allowing vendor URL overrides', () => {
    const resource = createEmpty()
    resource.spec = { type: 'httpdns', config: { preset: 'tencent', urlTemplate: 'https://resolver.example/?name={domain}', fallback: { type: 'dns', servers: [] } } }
    expect(() => validateLinkSys(resource)).toThrow('fallback')
    const config = resource.spec.config as HttpDnsConfig
    config.fallback!.servers = ['[::1]:5353', '1.1.1.1']
    expect(() => validateLinkSys(resource)).not.toThrow()
  })

  it('requires the Controller response-byte floor for Webhook body predicates', () => {
    const resource = createEmpty()
    resource.spec = { type: 'webhook', config: { target: { url: 'https://hook.example' }, maxResponseBytes: 4095, success: { body: [{ pointer: '/ok', equals: true }] } } }
    expect(() => validateLinkSys(resource)).toThrow('4096')
    const config = resource.spec.config as WebhookConfig
    config.maxResponseBytes = 4096
    expect(() => validateLinkSys(resource)).not.toThrow()
  })
})
