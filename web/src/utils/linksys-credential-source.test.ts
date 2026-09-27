import { describe, expect, it } from 'vitest'
import { createConfig, createEmpty, fromYaml, toMutationYaml, validateLinkSys } from './linksys'
import type { CredentialSourceConfig } from '@/types/link-sys'

function fixture() {
  const resource = createEmpty()
  const config = createConfig('credentialSource') as CredentialSourceConfig
  config.provider.tokenEndpoint = 'https://issuer.example/token'
  config.provider.clientAuthentication.activeSecretRef = { name: 'bootstrap', namespace: 'edge' }
  resource.spec = { type: 'credentialSource', config }
  return { resource, config }
}

describe('credentialSource LinkSys', () => {
  it('preserves absent defaults, false switches and references while filtering resolved TLS', () => {
    const { resource, config } = fixture()
    config.provider.scopes = ['read:items', 'write:items']
    config.provider.clientAuthentication.previousSecretRef = { name: 'previous' }
    config.provider.tls = { enabled: false, resolvedCaCertificates: ['[redacted]'], resolvedClientCertificate: '[redacted]' } as any
    config.publication = { persist: false, memoryMaxKeys: 1 }
    config.egress = { blockPrivate: false }
    expect(() => validateLinkSys(resource)).not.toThrow()
    const mutation = fromYaml(toMutationYaml(resource, 'create'))
    expect(mutation.spec.config).not.toHaveProperty('rotation')
    expect(mutation.spec.config).not.toHaveProperty('provider.tls.resolvedCaCertificates')
    expect(mutation.spec.config).not.toHaveProperty('provider.tls.resolvedClientCertificate')
    expect(mutation.spec.config).toMatchObject({ publication: { persist: false, memoryMaxKeys: 1 }, egress: { blockPrivate: false }, provider: { clientAuthentication: config.provider.clientAuthentication, scopes: config.provider.scopes } })
  })

  it('rejects unsupported issuers and credential identities', () => {
    const { resource, config } = fixture()
    for (const endpoint of ['http://issuer/token', 'https://user:pass@issuer/token', 'https://issuer/token?', 'https://issuer/token#']) {
      config.provider.tokenEndpoint = endpoint
      expect(() => validateLinkSys(resource)).toThrow('tokenEndpoint')
    }
    config.provider.tokenEndpoint = 'https://issuer.example/token'
    config.provider.clientAuthentication.previousSecretRef = { ...config.provider.clientAuthentication.activeSecretRef }
    expect(() => validateLinkSys(resource)).toThrow('must differ')
    delete config.provider.clientAuthentication.previousSecretRef
    config.provider.clientAuthentication.activeSecretRef.kind = 'ConfigMap'
    expect(() => validateLinkSys(resource)).toThrow('core Secret')
  })

  it('validates scope grammar, count, uniqueness and aggregate size', () => {
    const { resource, config } = fixture()
    for (const scopes of [['two words'], ['bad"scope'], ['back\\slash'], [''], ['same', 'same'], Array.from({ length: 17 }, (_, i) => `scope${i}`), Array.from({ length: 9 }, (_, i) => `${i}${'x'.repeat(255)}`)]) {
      config.provider.scopes = scopes
      expect(() => validateLinkSys(resource)).toThrow('scopes')
    }
  })

  it('enforces duration boundaries, retry ordering and publication limits', () => {
    const { resource, config } = fixture()
    for (const rotation of [{ interval: '29s' }, { interval: '25h' }, { requestTimeout: '999ms' }, { requestTimeout: '61s' }, { retryInitialBackoff: '61s' }, { retryMaxBackoff: '16m' }, { retryInitialBackoff: '10s', retryMaxBackoff: '5s' }]) {
      config.rotation = rotation
      expect(() => validateLinkSys(resource)).toThrow('rotation')
    }
    config.rotation = { interval: '30s', requestTimeout: '1s', retryInitialBackoff: '1s', retryMaxBackoff: '15m' }
    expect(() => validateLinkSys(resource)).not.toThrow()
    for (const memoryMaxKeys of [0, 10001, 1.5]) {
      config.publication = { memoryMaxKeys }
      expect(() => validateLinkSys(resource)).toThrow('memoryMaxKeys')
    }
  })
})
