import { describe, expect, it } from 'vitest'
import {
  fromYaml,
  normalize,
  replaceChallengeType,
  toMutationDocument,
  toYaml,
} from './edgionacme'
import type { EdgionAcme } from '@/types/edgion-acme'

const fullDnsFixture: EdgionAcme = {
  apiVersion: 'edgion.io/v1',
  kind: 'EdgionAcme',
  metadata: { name: 'production', namespace: 'edge', labels: { purpose: '' } },
  spec: {
    server: 'https://acme.example/directory',
    email: 'ops@example.com',
    privateKeySecretRef: { name: 'account', namespace: 'secrets', group: '', kind: 'Secret' },
    domains: ['example.com', '*.example.com'],
    keyType: 'ecdsa-p384',
    challenge: {
      type: 'dns-01', provider: 'cloudflare',
      credentialRef: { name: 'dns', namespace: 'secrets' },
      propagationTimeout: 0, propagationCheckInterval: 5,
      futureChallengeField: false,
    },
    renewal: { renewBeforeDays: 30, checkInterval: 86400, failBackoff: 0, futureRenewal: '' },
    externalAccountBinding: {
      keyId: 'kid', keySecretRef: { name: 'eab', namespace: 'secrets' },
    },
    storage: { secretName: 'certificate', secretNamespace: 'edge', futureStorage: [] },
    autoEdgionTls: {
      enabled: false, name: '',
      parentRefs: [{ name: 'gateway', namespace: 'edge', sectionName: 'https', port: 443 }],
    },
    futureSpecField: { empty: {}, disabled: false },
  },
}

const fullHttpFixture: EdgionAcme = {
  ...fullDnsFixture,
  metadata: { name: 'staging', namespace: 'edge' },
  spec: {
    ...fullDnsFixture.spec,
    keyType: 'ecdsa-p256',
    challenge: {
      type: 'http-01',
      gatewayRef: { name: 'gateway', namespace: 'edge', sectionName: '', futureRefField: false },
      futureChallengeField: [],
    },
  },
}

describe('EdgionAcme resource adapter', () => {
  it.each([['dns-01', fullDnsFixture], ['http-01', fullHttpFixture]] as const)(
    'round-trips the full flat %s fixture without injecting defaults',
    (_type, fixture) => {
      expect(fromYaml(toYaml(fixture, 'update'))).toEqual(fixture)
    },
  )

  it('preserves operator fields but strips status and server metadata on YAML-tab submission', () => {
    const apiView = {
      ...fullDnsFixture,
      metadata: {
        ...fullDnsFixture.metadata,
        uid: 'uid', resourceVersion: '7', creationTimestamp: '2026-01-01T00:00:00Z',
        managedFields: [],
      },
      status: { phase: 'Ready', activeChallenges: [{ token: '[redacted]' }] },
    }
    expect(normalize(apiView)).toBe(apiView)
    const yamlTabDocument = fromYaml(toYaml(apiView, 'update'))
    expect(toMutationDocument(yamlTabDocument, 'update')).toEqual({
      apiVersion: 'edgion.io/v1', kind: 'EdgionAcme',
      metadata: { name: 'production', namespace: 'edge', labels: { purpose: '' }, resourceVersion: '7' },
      spec: fullDnsFixture.spec,
    })
  })

  it('switches challenge variants without nesting and retains unknown sibling fields', () => {
    const http = replaceChallengeType(fullDnsFixture.spec.challenge, 'http-01')
    expect(http).toEqual({
      type: 'http-01', gatewayRef: { name: '' }, futureChallengeField: false,
    })
    expect(http).not.toHaveProperty('dns01')
    const dns = replaceChallengeType(fullHttpFixture.spec.challenge, 'dns-01')
    expect(dns).toEqual({
      type: 'dns-01', provider: '', credentialRef: { name: '' }, futureChallengeField: [],
    })
    expect(dns).not.toHaveProperty('http01')
  })

  it('rejects the obsolete nested challenge shape', () => {
    const legacy = `apiVersion: edgion.io/v1\nkind: EdgionAcme\nmetadata: {name: bad}\nspec:\n  challenge:\n    type: http-01\n    http01: {gatewayRef: {name: gateway}}\n`
    expect(() => fromYaml(legacy)).toThrow(/flat challenge.gatewayRef/)
  })
})
