import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { createEmptyEdgionBackend, edgionBackendFromYaml, edgionBackendToMutationYaml, edgionBackendToYaml, validateEdgionBackend } from './edgionbackend'

export function backendFixture() {
  const value = createEmptyEdgionBackend()
  value.metadata = { name: 'provider', namespace: 'edge', resourceVersion: '42' }
  value.spec.ai.endpoint = 'https://provider.example.test'
  value.spec.ai.credentialPool.credentials = [
    { name: 'primary', secretRef: { name: 'key-one' }, secret: '[redacted]', weight: 3 },
    { name: 'secondary', secretRef: { name: 'key-two', namespace: 'other' }, weight: 1 },
  ]
  value.spec.ai.models = [
    { name: 'model-one', aliases: ['default'], public: false, pricing: { inputPerMillionUsd: '0.000001', outputPerMillionUsd: '2.50' } },
    { name: 'model-two', aliases: ['fast'], public: true },
  ]
  value.spec.ai.defaults = { streaming: false, maxOutputTokens: 2048 }
  value.spec.currentStatus = { conditions: [{ type: 'Accepted', status: 'True' }] }
  value.spec.futureOption = { enabled: false }
  value.status = { controllers: [{ controllerName: 'east', status: { conditions: [] } }] }
  return value
}

describe('EdgionBackend documents', () => {
  it('round-trips provider fields, multiple credentials/models, decimal strings and unknown fields', () => {
    const value = backendFixture()
    expect(edgionBackendFromYaml(edgionBackendToYaml(value))).toEqual(value)
    expect(validateEdgionBackend(value)).toEqual([])
  })

  it.each(['create', 'update'] as const)('removes resolved secrets and runtime state on %s', (mode) => {
    const value = backendFixture()
    const payload = yaml.load(edgionBackendToMutationYaml(value, mode)) as typeof value
    expect(payload.status).toBeUndefined()
    expect(payload.spec.currentStatus).toBeUndefined()
    expect(payload.spec.ai.credentialPool.credentials[0].secret).toBeUndefined()
    expect(payload.spec.ai.credentialPool.credentials[0].secretRef).toEqual({ name: 'key-one' })
    expect(payload.spec.ai.models).toEqual(value.spec.ai.models)
    expect(payload.spec.futureOption).toEqual({ enabled: false })
    expect(payload.metadata.resourceVersion).toBe(mode === 'update' ? '42' : undefined)
    expect(value.spec.ai.credentialPool.credentials[0].secret).toBe('[redacted]')
  })

  it('requires explicit insecure HTTP opt-in and an endpoint for compatible providers', () => {
    const value = backendFixture()
    value.spec.ai.provider = 'OpenAICompatible'
    delete value.spec.ai.endpoint
    expect(validateEdgionBackend(value)).toContain('OpenAICompatible requires an endpoint')
    value.spec.ai.endpoint = 'http://provider.example.test'
    expect(validateEdgionBackend(value)).toContain('Endpoint must use HTTPS unless insecure HTTP is explicitly enabled')
    value.spec.ai.allowInsecureHttp = true
    expect(validateEdgionBackend(value)).toEqual([])
  })

  it('rejects malformed editor shapes instead of silently dropping entries', () => {
    const value = backendFixture()
    expect(() => edgionBackendFromYaml(yaml.dump({ ...value, spec: { ai: {} } }))).toThrow()
    expect(() => edgionBackendFromYaml(yaml.dump({ ...value, kind: 'Service' }))).toThrow()
  })
})
