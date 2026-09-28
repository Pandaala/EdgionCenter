import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { normalize, toMutationYaml, validateBackendTLSPolicy } from './backendtlspolicy'

describe('BackendTLSPolicy validation', () => {
  it('supports AI targets and rejects invalid attachment and trust combinations', () => {
    const policy = normalize({ apiVersion: 'gateway.networking.k8s.io/v1', kind: 'BackendTLSPolicy', metadata: { name: 'ai', namespace: 'prod' }, spec: { targetRefs: [{ group: 'edgion.io', kind: 'EdgionBackend', name: 'provider' }], validation: { hostname: 'provider.example.com', wellKnownCACertificates: 'System' }, resolvedTargetClass: 'Ai', futureField: { enabled: false } } })
    const mutation = yaml.load(toMutationYaml(policy, 'create')) as any
    expect(mutation.spec.targetRefs).toEqual(policy.spec.targetRefs)
    expect(mutation.spec.resolvedTargetClass).toBeUndefined()
    expect(mutation.spec.futureField).toEqual({ enabled: false })
    policy.spec.targetRefs[0].sectionName = 'https'
    expect(() => validateBackendTLSPolicy(policy)).toThrow('sectionName')
    delete policy.spec.targetRefs[0].sectionName
    policy.spec.targetRefs.push({ group: '', kind: 'Service', name: 'extra' })
    expect(() => validateBackendTLSPolicy(policy)).toThrow('Exactly one')
    policy.spec.targetRefs.pop()
    policy.spec.validation.caCertificateRefs = [{ group: '', kind: 'Secret', name: 'ca' }]
    expect(() => validateBackendTLSPolicy(policy)).toThrow('mutually exclusive')
    delete policy.spec.validation.wellKnownCACertificates
    policy.spec.validation.caCertificateRefs.push({ group: 'core', kind: 'Secret', name: 'ca' })
    expect(() => validateBackendTLSPolicy(policy)).toThrow('Duplicate')
  })

  it('supports section refs, system CA, SANs and client certificate option', () => {
    const policy = normalize({ apiVersion:'gateway.networking.k8s.io/v1',kind:'BackendTLSPolicy',metadata:{name:'api',namespace:'prod'},spec:{targetRefs:[{group:'',kind:'Service',name:'api',sectionName:'https'}],validation:{hostname:'api.internal',wellKnownCACertificates:'System',subjectAltNames:[{type:'URI',uri:'spiffe://prod/api'}]},options:{'edgion.io/client-certificate-ref':'client-cert'}} })
    expect(() => validateBackendTLSPolicy(policy)).not.toThrow()
    expect((yaml.load(toMutationYaml(policy,'create')) as any).spec.targetRefs[0].sectionName).toBe('https')
  })

  it('rejects namespace-qualified client certificates and cross-namespace CA references', () => {
    const policy = normalize({apiVersion:'gateway.networking.k8s.io/v1',kind:'BackendTLSPolicy',metadata:{name:'api',namespace:'prod'},spec:{targetRefs:[{group:'',kind:'Service',name:'api'}],validation:{hostname:'api.internal',caCertificateRefs:[{group:'',kind:'Secret',name:'ca',namespace:'pki'}]},options:{'edgion.io/client-certificate-ref':'pki/client'}}})
    delete policy.spec.validation.caCertificateRefs![0].namespace
    expect(() => validateBackendTLSPolicy(policy)).toThrow('bare Secret name')
    policy.spec.options!['edgion.io/client-certificate-ref']='client'
    policy.spec.validation.caCertificateRefs![0].namespace = 'pki'
    expect(() => toMutationYaml(policy, 'update')).toThrow('policy namespace')
    delete policy.spec.validation.caCertificateRefs![0].namespace
    expect(() => toMutationYaml(policy, 'update')).not.toThrow()
  })

  it('rejects missing trust and malformed CA reference kinds', () => {
    const base:any={apiVersion:'gateway.networking.k8s.io/v1',kind:'BackendTLSPolicy',metadata:{name:'api',namespace:'prod'},spec:{targetRefs:[{group:'',kind:'Service',name:'api'}],validation:{hostname:'api.internal'}}}
    expect(() => validateBackendTLSPolicy(normalize(base))).toThrow('Choose CA')
    base.spec.validation.caCertificateRefs=[{group:'',kind:'Other',name:'ca'}]
    expect(() => validateBackendTLSPolicy(normalize(base))).toThrow('Secret or ConfigMap')
  })
})


describe('BackendTLSPolicy identity admission', () => {
  const makePolicy = () => normalize({ apiVersion: 'gateway.networking.k8s.io/v1', kind: 'BackendTLSPolicy', metadata: { name: 'api', namespace: 'prod' }, spec: { targetRefs: [{ group: '', kind: 'Service', name: 'api' }], validation: { hostname: 'api.internal', wellKnownCACertificates: 'System' } } })

  it.each(['*.internal', 'UPPER.internal', 'api.internal\n', 'a'.repeat(254)])('rejects invalid SNI %j', (hostname) => {
    const policy = makePolicy()
    policy.spec.validation.hostname = hostname
    expect(() => validateBackendTLSPolicy(policy)).toThrow('precise hostname')
  })

  it.each([[], Array(6).fill({ type: 'Hostname', hostname: 'api.internal' }), [{ type: 'DNS', hostname: 'api.internal' }], [{ type: 'Hostname', hostname: 'api.internal', uri: 'spiffe://prod/api' }], [{ type: 'URI', uri: '/relative' }], [{ type: 'URI', uri: 'urn:' + 'x'.repeat(250) }]].map(sans => ({ sans })))('rejects malformed SAN lists $sans', ({ sans }) => {
    const policy = makePolicy()
    policy.spec.validation.subjectAltNames = sans as any
    expect(() => validateBackendTLSPolicy(policy)).toThrow()
  })

  it('accepts wildcard DNS and absolute non-HTTP URI SANs', () => {
    const policy = makePolicy()
    policy.spec.validation.subjectAltNames = [{ type: 'Hostname', hostname: '*.internal' }, { type: 'URI', uri: 'urn:example:client' }]
    expect(() => validateBackendTLSPolicy(policy)).not.toThrow()
    delete policy.spec.validation.subjectAltNames
    expect(() => validateBackendTLSPolicy(policy)).not.toThrow()
  })
})


describe('BackendTLSPolicy client certificate names', () => {
  const policyWith = (value: unknown) => normalize({
    apiVersion: 'gateway.networking.k8s.io/v1', kind: 'BackendTLSPolicy',
    metadata: { name: 'api', namespace: 'prod' },
    spec: { targetRefs: [{ group: '', kind: 'Service', name: 'api' }],
      validation: { hostname: 'api.internal', wellKnownCACertificates: 'System' },
      options: { 'edgion.io/client-certificate-ref': value } },
  })

  it.each(['', '   ', 'a..b', 'a.-b', 'a-.b', 'Upper', 'ns/cert', 'a'.repeat(64),
    Array(4).fill('a'.repeat(63)).join('.'), null, 12])('rejects invalid option %j', value => {
    expect(() => toMutationYaml(policyWith(value), 'update')).toThrow('bare Secret name')
  })

  it.each(['client-cert', 'a.b', 'a'.repeat(63),
    [...Array(3).fill('a'.repeat(63)), 'b'.repeat(61)].join('.'), '  client-cert\n'])('preserves accepted option %j', value => {
    const mutation = yaml.load(toMutationYaml(policyWith(value), 'update')) as any
    expect(mutation.spec.options['edgion.io/client-certificate-ref']).toBe(value)
  })
})


describe('BackendTLSPolicy CA reference bounds', () => {
  it('accepts eight references and rejects nine without changing the draft', () => {
    const policy = normalize({ apiVersion: 'gateway.networking.k8s.io/v1', kind: 'BackendTLSPolicy',
      metadata: { name: 'api', namespace: 'prod' }, spec: {
        targetRefs: [{ group: '', kind: 'Service', name: 'api' }],
        validation: { hostname: 'api.internal', caCertificateRefs: Array.from({ length: 8 }, (_, i) => ({ group: '', kind: 'ConfigMap', name: `ca-${i}` })) },
      },
    })
    expect(() => toMutationYaml(policy, 'create')).not.toThrow()
    policy.spec.validation.caCertificateRefs!.push({ group: '', kind: 'Secret', name: 'extra' })
    const before = structuredClone(policy)
    expect(() => toMutationYaml(policy, 'update')).toThrow('at most eight')
    expect(policy).toEqual(before)
    policy.spec.validation.caCertificateRefs = {} as any
    expect(() => toMutationYaml(policy, 'create')).toThrow('must be an array')
  })
})
