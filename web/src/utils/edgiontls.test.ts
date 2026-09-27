import { describe, expect, it } from 'vitest'
import { normalizeEdgionTls, toMutationDocument, validateEdgionTls, createEmptyEdgionTls } from './edgiontls'

describe('EdgionTls lossless adapter', () => {
  it('uses ciphers, preserves unknown operator fields, and strips resolved/internal fields', () => {
    const fixture = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionTls' as const,
      metadata: { name: 'tls', namespace: 'edge', resourceVersion: '5' },
      spec: {
        parentRefs: [{ name: 'gateway', sectionName: 'https', futureRef: true }],
        hosts: ['example.com', '*.example.com'],
        secretRef: { name: 'certificate' },
        clientAuth: { mode: 'Mutual', caSecretRef: { name: 'ca' }, caSecret: { data: 'runtime' }, allowedSans: [] },
        minTlsVersion: 'TLS1_2',
        ciphers: ['ECDHE-RSA-AES256-GCM-SHA384'],
        secret: { data: 'runtime' },
        resolvedListeners: ['runtime'],
        resolvedLogLabels: { runtime: 'value' },
        futureTls: false,
      },
      status: { parents: [] },
    }

    const view = normalizeEdgionTls(fixture)
    const mutation = toMutationDocument(view, 'update')

    expect(view).toEqual(fixture)
    expect(mutation).toHaveProperty('spec.ciphers.0', 'ECDHE-RSA-AES256-GCM-SHA384')
    expect(mutation).toHaveProperty('spec.clientAuth.allowedSans', [])
    expect(mutation).toHaveProperty('spec.futureTls', false)
    expect(mutation).not.toHaveProperty('spec.clientAuth.caSecret')
    expect(mutation).not.toHaveProperty('spec.secret')
    expect(mutation).not.toHaveProperty('spec.resolvedListeners')
    expect(mutation).not.toHaveProperty('spec.resolvedLogLabels')
    expect(mutation).not.toHaveProperty('status')
  })
})

it('preserves typed SAN matchers and validates their structural bounds', () => {
  const resource = createEmptyEdgionTls()
  resource.spec.clientAuth = { mode: 'Mutual', caSecretRef: { name: 'ca' }, verifyDepth: 2, allowedCns: ['client'], allowedSans: [
    { type: 'URI', match: 'Prefix', value: 'spiffe://cluster/' },
    { type: 'OtherName', value: 'client', oid: '1.2.3.4', ignoreCase: true },
    { type: 'DNS', match: 'RegularExpression', value: '^client[0-9]+$' },
  ] }
  expect(validateEdgionTls(resource)).toEqual([])
  expect(toMutationDocument(normalizeEdgionTls(resource), 'create')).toHaveProperty('spec.clientAuth.allowedSans', resource.spec.clientAuth.allowedSans)
  resource.spec.clientAuth.allowedSans![1].oid = '1.02.3'
  expect(validateEdgionTls(resource).join(' ')).toContain('oid')
  resource.spec.clientAuth.allowedSans = ['old-string'] as any
  expect(validateEdgionTls(resource).join(' ')).toContain('typed SAN')
  expect(() => normalizeEdgionTls(resource)).toThrow('typed SAN')
})
