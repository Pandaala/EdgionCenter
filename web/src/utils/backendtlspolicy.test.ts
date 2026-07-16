import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { normalize, toMutationYaml, validateBackendTLSPolicy } from './backendtlspolicy'

describe('BackendTLSPolicy validation', () => {
  it('supports section refs, system CA, SANs and client certificate option', () => {
    const policy = normalize({ apiVersion:'gateway.networking.k8s.io/v1',kind:'BackendTLSPolicy',metadata:{name:'api',namespace:'prod'},spec:{targetRefs:[{group:'',kind:'Service',name:'api',sectionName:'https'}],validation:{hostname:'api.internal',wellKnownCACertificates:'System',subjectAltNames:[{type:'URI',uri:'spiffe://prod/api'}]},options:{'edgion.io/client-certificate-ref':'client-cert'}} })
    expect(() => validateBackendTLSPolicy(policy)).not.toThrow()
    expect((yaml.load(toMutationYaml(policy,'create')) as any).spec.targetRefs[0].sectionName).toBe('https')
  })

  it('rejects namespace-qualified client certificates but preserves CA ref namespaces', () => {
    const policy = normalize({apiVersion:'gateway.networking.k8s.io/v1',kind:'BackendTLSPolicy',metadata:{name:'api',namespace:'prod'},spec:{targetRefs:[{group:'',kind:'Service',name:'api'}],validation:{hostname:'api.internal',caCertificateRefs:[{group:'',kind:'Secret',name:'ca',namespace:'pki'}]},options:{'edgion.io/client-certificate-ref':'pki/client'}}})
    expect(() => validateBackendTLSPolicy(policy)).toThrow('bare Secret name')
    policy.spec.options!['edgion.io/client-certificate-ref']='client'
    expect((yaml.load(toMutationYaml(policy,'update')) as any).spec.validation.caCertificateRefs[0].namespace).toBe('pki')
  })

  it('rejects missing trust and malformed CA reference kinds', () => {
    const base:any={apiVersion:'gateway.networking.k8s.io/v1',kind:'BackendTLSPolicy',metadata:{name:'api',namespace:'prod'},spec:{targetRefs:[{group:'',kind:'Service',name:'api'}],validation:{hostname:'api.internal'}}}
    expect(() => validateBackendTLSPolicy(normalize(base))).toThrow('Choose CA')
    base.spec.validation.caCertificateRefs=[{group:'',kind:'Other',name:'ca'}]
    expect(() => validateBackendTLSPolicy(normalize(base))).toThrow('Secret or ConfigMap')
  })
})
