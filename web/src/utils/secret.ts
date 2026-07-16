/**
 * Secret 工具函数
 */

import * as yaml from 'js-yaml'
import { mutationDocumentToYaml } from './resource-document'

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
  stringData?: Record<string, string>
  immutable?: boolean
  [key: string]: unknown
}

export function createEmpty(): SecretResource {
  return {
    apiVersion: 'v1',
    kind: 'Secret',
    metadata: { name: '', namespace: 'default' },
    type: 'Opaque',
    stringData: {},
  }
}

export function normalize(raw: any): SecretResource {
  return {
    ...raw,
    apiVersion: raw.apiVersion || 'v1',
    kind: 'Secret',
    metadata: {
      ...raw.metadata,
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
    },
    type: raw.type || 'Opaque',
    data: raw.data,
  }
}

export function assertNoRedactionSentinel(secret: SecretResource): void {
  for (const field of ['data', 'stringData'] as const) {
    const values = secret[field]
    if (values && Object.values(values).some((value) => /redacted/i.test(value))) {
      throw new Error(`Secret ${field} contains a redacted value; replace it before saving`)
    }
  }
}

export function createWriteOnlyReplacement(resource: Pick<SecretResource, 'metadata'>): SecretResource {
  return {
    apiVersion: 'v1',
    kind: 'Secret',
    metadata: {
      name: resource.metadata.name,
      namespace: resource.metadata.namespace || 'default',
      labels: resource.metadata.labels,
      annotations: resource.metadata.annotations,
      resourceVersion: resource.metadata.resourceVersion,
    },
    type: 'Opaque',
    stringData: {},
  }
}

export function validateSecretWrite(secret: SecretResource): void {
  assertNoRedactionSentinel(secret)
  const values = [...Object.values(secret.data ?? {}), ...Object.values(secret.stringData ?? {})]
  if (values.length === 0 || values.every((value) => value.length === 0)) {
    throw new Error('Enter at least one new Secret value before saving')
  }
}

export function toYaml(secret: SecretResource, mode: 'create' | 'update' = 'update'): string {
  assertNoRedactionSentinel(secret)
  return mutationDocumentToYaml(secret, 'secret', mode)
}

export function fromYaml(yamlStr: string): SecretResource {
  const raw = yaml.load(yamlStr) as any
  return normalize(raw)
}
