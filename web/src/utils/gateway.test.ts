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
        secrets: '[redacted]',
        resolvedFrontendCaSecrets: '[redacted]',
      },
    }],
    tls: { backend: { clientCertificateRef: { name: 'client', namespace: 'certs', kind: 'Secret' } }, frontend: { default: { validation: { mode: 'AllowValidOnly', caCertificateRefs: [{ name: 'default-ca', kind: 'ConfigMap' }] } }, perPort: [{ port: 443, tls: { validation: { mode: 'AllowInsecureFallback', caCertificateRefs: [{ name: 'port-ca', kind: 'Secret' }] } }, futurePort: true }] } },
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
    expect(fromForm.spec.listeners[0].tls.secrets).toBeUndefined()
    expect(fromForm.spec.listeners[0].tls.resolvedFrontendCaSecrets).toBeUndefined()
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
