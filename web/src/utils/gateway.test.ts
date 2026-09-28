import * as yaml from 'js-yaml'
import { describe, expect, it } from 'vitest'
import { gatewayToMutationYaml, gatewayToYaml, normalizeGateway, validateGateway, yamlToGateway } from './gateway'

const fixture: any = {
  apiVersion: 'gateway.networking.k8s.io/v1',
  kind: 'Gateway',
  metadata: { name: 'edge', namespace: 'prod', labels: { owner: 'platform' }, resourceVersion: '8' },
  spec: {
    gatewayClassName: 'edgion',
    addresses: [{ type: 'NamedAddress', value: 'public', futureAddress: false }, { type: 'networking.example.io/static', value: 'edge.example.com' }],
    listeners: [{
      name: 'https', hostname: '*.example.com', port: 443, protocol: 'HTTPS', futureListener: [],
      allowedRoutes: { namespaces: { from: 'Selector', selector: { matchLabels: { tenant: 'blue' }, matchExpressions: [{ key: 'environment', operator: 'In', values: ['prod', 'stage'], futureExpression: false }], futureSelector: true } }, kinds: [{ group: 'gateway.networking.k8s.io', kind: 'HTTPRoute' }, { kind: 'GRPCRoute' }] },
      tls: {
        mode: 'Terminate',
        certificateRefs: [{ group: '', kind: 'Secret', namespace: 'certs', name: 'one' }, { group: 'edgion.io', kind: 'EdgionTls', name: 'two' }],
        frontendValidation: { mode: 'AllowInsecureFallback', caCertificateRefs: [{ kind: 'ConfigMap', namespace: 'certs', name: 'ca' }] },
        options: { 'edgion.io/cert-provider': 'edgion-tls', futureOption: '' },
        resolvedCertificateRefs: '[redacted]',
        resolvedFrontendCaRefs: '[redacted]',
      },
    }],
    tls: { backend: { clientCertificateRef: { name: 'client', namespace: 'certs', kind: 'Secret' } }, frontend: { default: { validation: { mode: 'AllowValidOnly', caCertificateRefs: [{ name: 'default-ca', group: '', kind: 'ConfigMap' }] } }, perPort: [{ port: 443, tls: { validation: { mode: 'AllowInsecureFallback', caCertificateRefs: [{ name: 'port-ca', group: '', kind: 'Secret' }] } }, futurePort: true }] } },
    futureSpec: { enabled: false },
  },
  status: { listeners: [{ name: 'https', attachedRoutes: 2 }] },
}

describe('Gateway lossless adapter', () => {
  it('round-trips all listener arrays, references, addresses, and unknown fields', () => {
    expect(yamlToGateway(gatewayToYaml(normalizeGateway(fixture)))).toEqual(fixture)
  })

  it('uses the same safe mutation boundary for form and YAML documents', () => {
    const fromForm = yaml.load(gatewayToMutationYaml(fixture, 'update')) as any
    const fromYaml = yaml.load(gatewayToMutationYaml(yamlToGateway(gatewayToYaml(fixture)), 'update')) as any
    expect(fromYaml).toEqual(fromForm)
    expect(fromForm.status).toBeUndefined()
    expect(fromForm.metadata.resourceVersion).toBe('8')
    expect(fromForm.spec.listeners).toHaveLength(1)
    expect(fromForm.spec.listeners[0].tls.certificateRefs).toHaveLength(2)
    expect(fromForm.spec.listeners[0].tls.resolvedCertificateRefs).toBeUndefined()
    expect(fromForm.spec.listeners[0].tls.resolvedFrontendCaRefs).toBeUndefined()
    expect(fromForm.spec.futureSpec).toEqual({ enabled: false })
  })

  it('validates listener, address, selector, reference, and global TLS constraints', () => {
    expect(validateGateway(fixture)).toEqual([])
    const invalid = structuredClone(fixture)
    invalid.spec.gatewayClassName = ''
    invalid.spec.addresses[0].type = 'Bad Type'
    invalid.spec.listeners[0].allowedRoutes.namespaces.selector.matchExpressions[0].values = []
    invalid.spec.tls.frontend.perPort.push({ port: 443, tls: { validation: { caCertificateRefs: [] } } })
    const errors = validateGateway(invalid).join('\n')
    expect(errors).toContain('gatewayClassName is required')
    expect(errors).toContain('type is invalid')
    expect(errors).toContain('values is required')
    expect(errors).toContain('port must be unique')
  })

  it('accepts options-only termination and a custom domain-prefixed protocol', () => {
    const resource = structuredClone(fixture)
    resource.spec.listeners = [
      { name: 'https-options', port: 443, protocol: 'HTTPS', tls: { mode: 'Terminate', options: { 'example.io/certificate-provider': 'external' } } },
      { name: 'custom', port: 9443, protocol: 'example.io/QUIC' },
    ]
    expect(validateGateway(resource)).toEqual([])
  })

  it('rejects every invalid built-in listener protocol combination from the v1.5 CRD', () => {
    const resource = structuredClone(fixture)
    resource.spec.listeners = [
      { name: 'http', port: 80, protocol: 'HTTP', tls: { mode: 'Terminate', options: { provider: 'x' } } },
      { name: 'https', port: 443, protocol: 'HTTPS', tls: { mode: 'Passthrough' } },
      { name: 'tls', port: 8443, protocol: 'TLS', tls: {} },
      { name: 'tcp', port: 9000, protocol: 'TCP', hostname: 'tcp.example.com' },
      { name: 'udp', port: 5353, protocol: 'UDP', hostname: 'udp.example.com' },
    ]
    const errors = validateGateway(resource).join('\n')
    expect(errors).toContain('tls must not be specified for HTTP')
    expect(errors).toContain('tls.mode must be Terminate for HTTPS')
    expect(errors).toContain('tls.mode must be explicitly set for TLS')
    expect(errors).toContain('hostname must not be specified for TCP')
    expect(errors).toContain('hostname must not be specified for UDP')
  })
})

it('strips Controller attachment proofs and rejects unsupported fields and oversized lists', () => {
  const resource = structuredClone(fixture)
  resource.spec.resolvedInboundProxyProtocol = { mode: 'trustedSources' }
  resource.spec.resolvedAttachmentProof = { instance: 'internal' }
  resource.spec.futureSpec.resolvedAttachmentProof = 'operator-value'
  const mutation = yaml.load(gatewayToMutationYaml(resource, 'update')) as any
  expect(mutation.spec).not.toHaveProperty('resolvedInboundProxyProtocol')
  expect(mutation.spec).not.toHaveProperty('resolvedAttachmentProof')
  expect(mutation.spec.futureSpec.resolvedAttachmentProof).toBe('operator-value')
  resource.spec.listeners = Array.from({ length: 65 }, (_, index) => ({ name: `http-${index}`, protocol: 'HTTP', port: 8000 + index }))
  resource.spec.tls.frontend.perPort = Array.from({ length: 65 }, (_, index) => ({ port: 8000 + index }))
  resource.spec.allowedListeners = {}
  const errors = validateGateway(resource).join(' ')
  expect(errors).toContain('spec.listeners must contain at most 64')
  expect(errors).toContain('spec.tls.frontend.perPort must contain at most 64')
  expect(errors).toContain('allowedListeners is not supported')
})

// Admin reads redact resolved certificate vectors; internal views carry arrays.
it.each(['create', 'update'] as const)('omits current listener TLS resolution fields on %s', (mode) => {
  const resource = structuredClone(fixture)
  const runtime = {
    resolvedCertificateRefs: '[redacted]',
    resolvedFrontendCaRefs: '[redacted]',
    frontendMatcherEligibility: 'StrictReject',
  }
  Object.assign(resource.spec.listeners[0].tls, runtime)
  resource.spec.listeners[0].tls.options = { ...runtime, 'example.io/provider': 'external' }
  const second = structuredClone(resource.spec.listeners[0])
  second.name = 'https-secondary'
  second.port = 8443
  second.tls.resolvedCertificateRefs = [{ index: 0, source: { group: '', kind: 'Secret', namespace: 'certs', name: 'serving' } }]
  second.tls.resolvedFrontendCaRefs = [{ index: 0, source: { kind: 'ConfigMap', namespace: 'certs', name: 'ca' } }]
  resource.spec.listeners.push(second, { name: 'http', port: 80, protocol: 'HTTP' })
  const before = structuredClone(resource)
  const mutation = yaml.load(gatewayToMutationYaml(resource, mode)) as any
  const fromYaml = yaml.load(gatewayToMutationYaml(yamlToGateway(gatewayToYaml(resource)), mode))
  expect(fromYaml).toEqual(mutation)
  for (const [index, listener] of mutation.spec.listeners.slice(0, 2).entries()) {
    const operatorTls = structuredClone(resource.spec.listeners[index].tls)
    delete operatorTls.resolvedCertificateRefs
    delete operatorTls.resolvedFrontendCaRefs
    delete operatorTls.frontendMatcherEligibility
    delete operatorTls.frontendValidation
    expect(listener.tls).toEqual(operatorTls)
  }
  expect(mutation.spec.listeners[2]).toEqual(resource.spec.listeners[2])
  expect(mutation.spec.tls).toEqual(resource.spec.tls)
  expect(mutation.metadata.resourceVersion).toBe(mode === 'update' ? '8' : undefined)
  expect(resource).toEqual(before)
})

it('rejects TLS termination without changing the operator document', () => {
  const resource = structuredClone(fixture)
  resource.spec.listeners[0].protocol = 'TLS'
  const before = structuredClone(resource)
  expect(validateGateway(resource)).toContain('spec.listeners[0].tls.mode must be Passthrough for TLS; the current Controller does not support TLS termination')
  expect(resource).toEqual(before)
  resource.spec.listeners[0].tls = { mode: 'Passthrough' }
  expect(validateGateway(resource)).toEqual([])
})


it('ignores malformed listener projections while validating the operator frontend policy', () => {
  const resource = structuredClone(fixture)
  resource.spec.listeners[0].tls.frontendValidation = { mode: 'invalid', caCertificateRefs: [] }
  expect(validateGateway(resource)).toEqual([])
  const mutation = yaml.load(gatewayToMutationYaml(resource, 'update')) as any
  expect(mutation.spec.listeners[0].tls).not.toHaveProperty('frontendValidation')
  expect(mutation.spec.tls.frontend).toEqual(resource.spec.tls.frontend)
  resource.spec.tls.frontend.default.validation.caCertificateRefs = []
  expect(validateGateway(resource)).toContain('spec.tls.frontend.default.validation.caCertificateRefs requires at least one reference')
})

describe('Gateway frontend CA admission', () => {
  const validRef = { group: '', kind: 'ConfigMap', name: 'client-ca' }
  it.each([
    [{ kind: 'Secret', name: 'ca' }, '.group'],
    [{ ...validRef, group: 'core' }, '.group'],
    [{ group: '', name: 'ca' }, '.kind'],
    [{ ...validRef, kind: '' }, '.kind'],
    [{ ...validRef, kind: '1Secret' }, '.kind'],
    [{ ...validRef, kind: 'S'.repeat(64) }, '.kind'],
    [{ ...validRef, name: '' }, '.name'],
    [{ ...validRef, name: 'a'.repeat(254) }, '.name'],
    [{ ...validRef, namespace: '' }, '.namespace'],
    [{ ...validRef, namespace: 'bad.namespace' }, '.namespace'],
    [{ ...validRef, namespace: 'a'.repeat(64) }, '.namespace'],
    [null, '.group'],
  ])('rejects malformed reference %j', (ref, field) => {
    const resource = structuredClone(fixture)
    resource.spec.tls.frontend.default.validation.caCertificateRefs = [ref]
    expect(validateGateway(resource).join(';')).toContain(`spec.tls.frontend.default.validation.caCertificateRefs[0]${field}`)
  })

  it('enforces bounds and required containers without rewriting drafts', () => {
    const resource = structuredClone(fixture)
    resource.spec.tls.frontend.default.validation.caCertificateRefs = Array(16).fill(validRef)
    expect(validateGateway(resource)).toEqual([])
    resource.spec.tls.frontend.default.validation.caCertificateRefs.push(validRef)
    expect(validateGateway(resource).join(';')).toContain('at most 16')
    resource.spec.tls.frontend = { perPort: [{ port: 443 }] }
    const before = structuredClone(resource)
    expect(validateGateway(resource)).toEqual([
      'spec.tls.frontend.default is required', 'spec.tls.frontend.perPort[0].tls is required',
    ])
    expect(resource).toEqual(before)
    resource.spec.tls.frontend = { default: {}, perPort: [{ port: 443, tls: {} }] }
    expect(validateGateway(resource)).toEqual([])
  })
})
