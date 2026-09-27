import { describe, expect, it } from 'vitest'
import {
  createEmpty,
  fromYaml,
  normalize,
  toEditableYaml,
  toMutationDocument,
  toYaml,
  validateEdgionAcme,
} from './edgionacme'
import type { EdgionAcme } from '@/types/edgion-acme'

const fullFixture: EdgionAcme = {
  apiVersion: 'edgion.io/v1',
  kind: 'EdgionAcme',
  metadata: { name: 'production', namespace: 'edge', labels: { purpose: '' } },
  spec: {
    server: 'https://acme.example/directory',
    email: 'ops@example.com',
    privateKeySecretRef: { name: 'account', namespace: 'secrets', group: '', kind: 'Secret' },
    domains: ['example.com', 'www.example.com'],
    keyType: 'ecdsa-p384',
    challenge: {
      type: 'http-01', gatewayRef: { name: 'gateway', namespace: 'edge' },
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
  ...fullFixture,
  metadata: { name: 'staging', namespace: 'edge' },
  spec: {
    ...fullFixture.spec,
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

  it.each([['http-01', fullFixture], ['http-01 scoped', fullHttpFixture]] as const)(
    'round-trips the full flat %s fixture without injecting defaults',
    (_type, fixture) => {
      expect(fromYaml(toYaml(fixture, 'update'))).toEqual(fixture)
    },
  )

  it('preserves operator fields but strips status and server metadata on YAML-tab submission', () => {
    const apiView = {
      ...fullFixture,
      metadata: {
        ...fullFixture.metadata,
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
      spec: fullFixture.spec,
    })
  })

  it('rejects unsupported DNS challenges and wildcard issuance', () => {
    const unsupported = structuredClone(fullFixture) as any
    unsupported.spec.challenge = { type: 'dns-01', provider: 'cloudflare', credentialRef: { name: 'dns' } }
    expect(() => normalize(unsupported)).toThrow('HTTP-01 only')
    expect(() => toMutationDocument(unsupported, 'create')).toThrow('HTTP-01 only')
    const wildcard = structuredClone(fullFixture)
    wildcard.spec.domains = ['*.example.com']
    expect(() => toMutationDocument(wildcard, 'create')).toThrow('wildcard')
  })

  it('serializes incomplete drafts but requires email and domains at submission', () => {
    const draft = createEmpty()
    expect(() => toEditableYaml(draft)).not.toThrow()
    expect(validateEdgionAcme(draft)).toEqual(['email is required', 'at least one domain is required'])
    expect(() => toMutationDocument(draft, 'create')).toThrow('email is required')
  })

  it('strips Controller attachment and notification data without removing nested operator values', () => {
    const resource = structuredClone(fullFixture)
    resource.spec.resolvedListenerAttachments = [{ gatewayName: 'internal' }]
    resource.spec.notifyAfterPublish = true
    resource.spec.futureSpecField = { resolvedListenerAttachments: ['retained'] }
    const mutation = toMutationDocument(resource, 'update')
    expect(mutation).not.toHaveProperty('spec.resolvedListenerAttachments')
    expect(mutation).not.toHaveProperty('spec.notifyAfterPublish')
    expect(mutation).toHaveProperty('spec.futureSpecField.resolvedListenerAttachments', ['retained'])
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
          type: 'http-01',
          gatewayRef: { name: 'gateway' },
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
    const resource = structuredClone(fullFixture)
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

  it('validates renewal durations while preserving unknown siblings', () => {
    const resource: EdgionAcme = {
      ...fullFixture,
      spec: {
        ...fullFixture.spec,
        challenge: {
          ...fullFixture.spec.challenge,
        },
        renewal: {
          ...fullFixture.spec.renewal,
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
