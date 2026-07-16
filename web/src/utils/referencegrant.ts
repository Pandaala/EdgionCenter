import * as yaml from 'js-yaml'
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

export function normalize(raw: any): ReferenceGrant {
  const spec = { ...raw.spec }
  if (Array.isArray(raw.spec?.from)) spec.from = raw.spec.from.map((entry: any) => ({ ...entry }))
  if (Array.isArray(raw.spec?.to)) spec.to = raw.spec.to.map((entry: any) => ({ ...entry }))
  return {
    ...raw,
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1',
    kind: 'ReferenceGrant',
    metadata: {
      ...raw.metadata,
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
    },
    spec: spec as ReferenceGrant['spec'],
  }
}

export function toYaml(rg: ReferenceGrant, mode: 'create' | 'update' = 'update'): string {
  return mutationDocumentToYaml(rg, 'referencegrant', mode)
}

export function fromYaml(yamlStr: string): ReferenceGrant {
  return normalize(yaml.load(yamlStr) as any)
}
