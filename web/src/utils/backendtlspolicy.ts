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
  if (!Array.isArray(policy.spec.targetRefs) || policy.spec.targetRefs.length !== 1) throw new Error('Exactly one targetRef is required')
  const target = policy.spec.targetRefs[0]
  if (!target.name) throw new Error('Target name is required')
  const service = ['', 'core'].includes(target.group) && target.kind === 'Service'
  const aiBackend = target.group === 'edgion.io' && target.kind === 'EdgionBackend'
  if (!service && !aiBackend) throw new Error('Target must be a core Service or edgion.io EdgionBackend')
  if (aiBackend && target.sectionName !== undefined) throw new Error('sectionName is not supported for EdgionBackend targets')
  // Keep these patterns aligned with edgion-resources gwapi_types.
  const hostnamePattern = /^(\*\.)?[a-z0-9]([-a-z0-9]*[a-z0-9])?(\.[a-z0-9]([-a-z0-9]*[a-z0-9])?)*$/
  const hostname = policy.spec.validation.hostname
  if (typeof hostname !== 'string' || hostname.length > 253 || hostname.startsWith('*.') || !hostnamePattern.test(hostname) || hostname.includes('\n')) {
    throw new Error('Validation hostname must be a valid precise hostname')
  }
  const refs = policy.spec.validation.caCertificateRefs ?? []
  if (!refs.length && policy.spec.validation.wellKnownCACertificates !== 'System') throw new Error('Choose CA references or the System CA bundle')
  if (refs.length && policy.spec.validation.wellKnownCACertificates !== undefined) throw new Error('CA references and the System CA bundle are mutually exclusive')
  const seenCaRefs = new Set<string>()
  refs.forEach((ref) => {
    if (!ref.name || !['Secret', 'ConfigMap'].includes(ref.kind) || !['', 'core'].includes(ref.group)) throw new Error('CA references must name a core Secret or ConfigMap')
    if (ref.namespace !== undefined) throw new Error('CA references must stay in the policy namespace; omit namespace')
    const key = `${ref.kind}/${ref.name}`
    if (seenCaRefs.has(key)) throw new Error('Duplicate CA references are not allowed')
    seenCaRefs.add(key)
  })
  const clientCert = policy.spec.options?.['edgion.io/client-certificate-ref']
  if (clientCert && (clientCert.includes('/') || !/^[a-z0-9]([-a-z0-9.]*[a-z0-9])?$/.test(clientCert))) {
    throw new Error('Client certificate reference must be a bare Secret name in the policy namespace')
  }
  const subjectAltNames = policy.spec.validation.subjectAltNames
  if (subjectAltNames === undefined) return
  if (!Array.isArray(subjectAltNames) || subjectAltNames.length < 1 || subjectAltNames.length > 5) {
    throw new Error('Subject alternative names must contain between one and five entries')
  }
  subjectAltNames.forEach((san) => {
    if (!san || (san.type !== 'Hostname' && san.type !== 'URI')) throw new Error('SAN type must be Hostname or URI')
    if (san.type === 'Hostname') {
      if (san.uri !== undefined || typeof san.hostname !== 'string' || san.hostname.length > 253 || !hostnamePattern.test(san.hostname) || san.hostname.includes('\n')) {
        throw new Error('Hostname SAN requires only a valid hostname')
      }
    } else {
      if (san.hostname !== undefined || typeof san.uri !== 'string' || new TextEncoder().encode(san.uri).length > 253) throw new Error('URI SAN requires only a valid absolute URI')
      try { new URL(san.uri) } catch { throw new Error('URI SAN requires only a valid absolute URI') }
    }
  })
}

export function fromYaml(yamlStr: string): BackendTLSPolicy {
  return normalize(yaml.load(yamlStr) as any)
}
