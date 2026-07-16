import * as yaml from 'js-yaml'
import { dumpYaml } from './yaml-utils'
import { mutationDocumentToYaml } from './resource-document'

export interface BackendTLSPolicyTargetRef {
  group: string
  kind: string
  name: string
  sectionName?: string
}

export interface BackendTLSPolicyCACertRef {
  name: string
  group: string
  kind: string
  namespace?: string
}

export interface BackendTLSPolicySubjectAltName {
  type: 'Hostname' | 'URI'
  hostname?: string
  uri?: string
}

export interface BackendTLSPolicy {
  apiVersion: string
  kind: string
  metadata: {
    name: string
    namespace?: string
    labels?: Record<string, string>
    annotations?: Record<string, string>
    resourceVersion?: string
    creationTimestamp?: string
  }
  spec: {
    targetRefs: BackendTLSPolicyTargetRef[]
    validation: {
      hostname: string
      caCertificateRefs?: BackendTLSPolicyCACertRef[]
      subjectAltNames?: BackendTLSPolicySubjectAltName[]
      wellKnownCACertificates?: 'System'
    }
    options?: Record<string, string>
    [key: string]: unknown
  }
  status?: any
}

export function createEmpty(): BackendTLSPolicy {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'BackendTLSPolicy',
    metadata: { name: '', namespace: 'default' },
    spec: {
      targetRefs: [{ group: '', kind: 'Service', name: '' }],
      validation: {
        hostname: '',
        caCertificateRefs: [{ name: '', group: '', kind: 'Secret' }],
      },
    },
  }
}

export function normalize(raw: unknown): BackendTLSPolicy {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('BackendTLSPolicy document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'BackendTLSPolicy') throw new Error('Expected a BackendTLSPolicy document')
  return structuredClone(document) as unknown as BackendTLSPolicy
}

export function toYaml(policy: BackendTLSPolicy): string {
  return dumpYaml(policy)
}

export function toMutationYaml(policy: BackendTLSPolicy, mode: 'create' | 'update'): string {
  validateBackendTLSPolicy(policy)
  return mutationDocumentToYaml(policy, 'backendtlspolicy', mode)
}

export function validateBackendTLSPolicy(policy: BackendTLSPolicy): void {
  if (!policy.metadata.name || !policy.metadata.namespace) throw new Error('Name and namespace are required')
  if (!policy.spec.targetRefs.length) throw new Error('At least one targetRef is required')
  policy.spec.targetRefs.forEach((ref) => { if (!ref.name || !ref.kind) throw new Error('Every targetRef needs name and kind') })
  if (!policy.spec.validation.hostname) throw new Error('Validation hostname is required')
  const refs = policy.spec.validation.caCertificateRefs ?? []
  if (!refs.length && policy.spec.validation.wellKnownCACertificates !== 'System') throw new Error('Choose CA references or the System CA bundle')
  refs.forEach((ref) => { if (!ref.name || !['Secret', 'ConfigMap'].includes(ref.kind)) throw new Error('CA references must name a Secret or ConfigMap') })
  const clientCert = policy.spec.options?.['edgion.io/client-certificate-ref']
  if (clientCert && (clientCert.includes('/') || !/^[a-z0-9]([-a-z0-9.]*[a-z0-9])?$/.test(clientCert))) {
    throw new Error('Client certificate reference must be a bare Secret name in the policy namespace')
  }
  const subjectAltNames = policy.spec.validation.subjectAltNames ?? []
  subjectAltNames.forEach((san) => {
    if (san.type === 'Hostname' && !san.hostname) throw new Error('Hostname SAN requires a hostname')
    if (san.type === 'URI' && !san.uri) throw new Error('URI SAN requires a URI')
  })
}

export function fromYaml(yamlStr: string): BackendTLSPolicy {
  return normalize(yaml.load(yamlStr) as any)
}
