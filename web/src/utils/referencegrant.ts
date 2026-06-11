import * as yaml from 'js-yaml'
import { dumpYaml } from './yaml-utils'

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
  const from: ReferenceGrantFrom[] = (raw.spec?.from || []).map((f: any) => ({
    group: f.group ?? 'gateway.networking.k8s.io',
    kind: f.kind || '',
    namespace: f.namespace || '',
  }))
  const to: ReferenceGrantTo[] = (raw.spec?.to || []).map((t: any) => ({
    group: t.group ?? '',
    kind: t.kind || '',
    name: t.name,
  }))
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1',
    kind: 'ReferenceGrant',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: {
      from: from.length > 0 ? from : [{ group: 'gateway.networking.k8s.io', kind: 'Gateway', namespace: '' }],
      to: to.length > 0 ? to : [{ group: '', kind: 'Secret' }],
    },
    status: raw.status,
  }
}

export function toYaml(rg: ReferenceGrant): string {
  return dumpYaml(rg)
}

export function fromYaml(yamlStr: string): ReferenceGrant {
  return normalize(yaml.load(yamlStr) as any)
}
