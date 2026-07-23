import { describe, expect, it } from 'vitest'
import {
  createEmpty,
  fromYaml,
  normalize,
  replaceChallengeType,
  toMutationDocument,
  toYaml,
  validateEdgionAcme,
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
      propagationTimeout: '120s', propagationCheckInterval: '5s',
      futureChallengeField: false,
    },
    renewal: { renewBefore: '720h', checkInterval: '24h', failBackoff: '5m', futureRenewal: '' },
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
  it('uses duration-string defaults only for newly created resources', () => {
    const created = createEmpty()
    expect(created.spec.renewal).toEqual({
      renewBefore: '720h',
      checkInterval: '24h',
      failBackoff: '5m',
    })
    expect(created.spec.renewal).not.toHaveProperty('renewBeforeDays')
    expect(created.spec.challenge).not.toHaveProperty('propagationTimeout')
    expect(created.spec.challenge).not.toHaveProperty('propagationCheckInterval')
  })

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
      type: 'dns-01',
      provider: '',
      credentialRef: { name: '' },
      propagationTimeout: '120s',
      propagationCheckInterval: '5s',
      futureChallengeField: [],
    })
    expect(dns).not.toHaveProperty('http01')
  })

  it('does not inject absent duration defaults into normalized existing documents', () => {
    const existing = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionAcme',
      metadata: { name: 'existing', namespace: 'edge' },
      spec: {
        email: 'ops@example.com',
        privateKeySecretRef: { name: 'account' },
        domains: ['example.com'],
        challenge: {
          type: 'dns-01',
          provider: 'cloudflare',
          credentialRef: { name: 'dns' },
          futureChallengeField: { retained: true },
        },
        renewal: { futureRenewal: false },
        storage: { secretName: 'certificate' },
        futureSpecField: ['retained'],
      },
    }
    const normalized = normalize(existing)
    expect(normalized).toBe(existing)
    expect(normalized.spec.challenge).not.toHaveProperty('propagationTimeout')
    expect(normalized.spec.challenge).not.toHaveProperty('propagationCheckInterval')
    expect(normalized.spec.renewal).not.toHaveProperty('renewBefore')
    expect(normalized.spec.renewal).not.toHaveProperty('checkInterval')
    expect(normalized.spec.renewal).not.toHaveProperty('failBackoff')
    expect(fromYaml(toYaml(normalized, 'update'))).toEqual(existing)
  })

  it.each([
    ['bare number', 30],
    ['bare numeric string', '30'],
    ['decimal', '1.5h'],
    ['days unit', '30d'],
  ])('rejects an invalid %s duration before mutation', (_label, invalidDuration) => {
    const resource = createEmpty()
    resource.spec.renewal = {
      ...resource.spec.renewal,
      renewBefore: invalidDuration as string,
    }
    expect(validateEdgionAcme(resource)).toEqual([
      'renewal.renewBefore must be a valid GEP-2257 duration',
    ])
    expect(() => toMutationDocument(resource, 'create')).toThrow(
      /renewal\.renewBefore must be a valid GEP-2257 duration/,
    )
  })

  it('validates every DNS and renewal duration while preserving unknown siblings', () => {
    const resource: EdgionAcme = {
      ...fullDnsFixture,
      spec: {
        ...fullDnsFixture.spec,
        challenge: {
          ...fullDnsFixture.spec.challenge,
          propagationTimeout: '2m30s',
          propagationCheckInterval: '500ms',
        },
        renewal: {
          ...fullDnsFixture.spec.renewal,
          renewBefore: '720h',
          checkInterval: '24h',
          failBackoff: '5m',
        },
      },
    }
    expect(validateEdgionAcme(resource)).toEqual([])
    expect(toMutationDocument(resource, 'update')).toMatchObject({
      spec: {
        challenge: { futureChallengeField: false },
        renewal: { futureRenewal: '' },
        futureSpecField: { empty: {}, disabled: false },
      },
    })
  })

  it('rejects the obsolete nested challenge shape', () => {
    const legacy = `apiVersion: edgion.io/v1\nkind: EdgionAcme\nmetadata: {name: bad}\nspec:\n  challenge:\n    type: http-01\n    http01: {gatewayRef: {name: gateway}}\n`
    expect(() => fromYaml(legacy)).toThrow(/flat challenge.gatewayRef/)
  })
})
