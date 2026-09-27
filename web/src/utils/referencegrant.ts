import * as yaml from 'js-yaml'
import { isValidDNS1123Label, isValidDNS1123Subdomain } from './validation'
import { mutationDocumentToYaml } from './resource-document'

export interface ReferenceGrantFrom {
  group: string
  kind: string
  namespace: string
}

export interface ReferenceGrantTo {
  group: string
  kind: string
  name?: string
}

export interface ReferenceGrant {
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
    from: ReferenceGrantFrom[]
    to: ReferenceGrantTo[]
  }
  status?: any
}

export function createEmpty(): ReferenceGrant {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'ReferenceGrant',
    metadata: { name: '', namespace: 'default' },
    spec: {
      from: [{ group: 'gateway.networking.k8s.io', kind: 'Gateway', namespace: '' }],
      to: [{ group: '', kind: 'Secret' }],
    },
  }
}

export function normalize(raw: unknown): ReferenceGrant {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('ReferenceGrant must be an object')
  const resource = raw as ReferenceGrant
  if (resource.kind !== 'ReferenceGrant') throw new Error('Expected ReferenceGrant kind')
  if (!resource.metadata || !resource.spec || typeof resource.spec !== 'object' || Array.isArray(resource.spec)) throw new Error('ReferenceGrant metadata and spec are required')
  return structuredClone(resource)
}

export function validateReferenceGrant(resource: ReferenceGrant): string[] {
  const errors: string[] = []
  for (const field of ['from', 'to'] as const) {
    const refs = resource.spec?.[field]
    if (!Array.isArray(refs) || refs.length < 1 || refs.length > 16) {
      errors.push(`spec.${field} must contain 1 to 16 references`)
      continue
    }
    refs.forEach((ref, index) => {
      const path = `spec.${field}[${index}]`
      if (!ref || typeof ref !== 'object') { errors.push(`${path} must be an object`); return }
      if (typeof ref.group !== 'string' || ref.group.length > 253 || (ref.group !== '' && !isValidDNS1123Subdomain(ref.group))) errors.push(`${path}.group must be a DNS subdomain or empty for the core group`)
      if (typeof ref.kind !== 'string' || ref.kind.length > 63 || !/^[a-zA-Z]([-a-zA-Z0-9]*[a-zA-Z0-9])?$/.test(ref.kind)) errors.push(`${path}.kind is invalid`)
      if (field === 'from') {
        const namespace = (ref as ReferenceGrantFrom).namespace
        if (typeof namespace !== 'string' || namespace.length > 63 || !isValidDNS1123Label(namespace)) errors.push(`${path}.namespace must be a DNS label`)
      }
    })
  }
  return errors
}

export function toYaml(rg: ReferenceGrant, mode: 'create' | 'update' = 'update'): string {
  return mutationDocumentToYaml(rg, 'referencegrant', mode)
}

export function fromYaml(yamlStr: string): ReferenceGrant {
  return normalize(yaml.load(yamlStr) as any)
}
