/**
 * Secret 工具函数
 */

import * as yaml from 'js-yaml'
import { removeEmpty } from './yaml-utils'

export interface SecretResource {
  apiVersion: string
  kind: 'Secret'
  metadata: {
    name: string
    namespace: string
    labels?: Record<string, string>
    annotations?: Record<string, string>
    resourceVersion?: string
    creationTimestamp?: string
  }
  type?: string
  data?: Record<string, string>
}

export function createEmpty(): SecretResource {
  return {
    apiVersion: 'v1',
    kind: 'Secret',
    metadata: { name: '', namespace: 'default' },
    type: 'Opaque',
    data: {},
  }
}

export function normalize(raw: any): SecretResource {
  return {
    apiVersion: raw.apiVersion || 'v1',
    kind: 'Secret',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    type: raw.type || 'Opaque',
    data: raw.data || {},
  }
}


export function toYaml(secret: SecretResource): string {
  const clean = removeEmpty(secret)
  return yaml.dump(clean, { lineWidth: -1, noRefs: true })
}

export function fromYaml(yamlStr: string): SecretResource {
  const raw = yaml.load(yamlStr) as any
  return normalize(raw)
}
