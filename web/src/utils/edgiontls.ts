/**
 * EdgionTls 工具函数
 */

import * as yaml from 'js-yaml'
import type { EdgionTls } from '@/types/edgion-tls'
import { dumpYaml } from './yaml-utils'
import { buildMutationDocument } from './resource-document'

export const DEFAULT_EDGIONTLS_YAML = `apiVersion: edgion.io/v1
kind: EdgionTls
metadata:
  name: example-tls
  namespace: default
spec:
  hosts:
    - "*.example.com"
  secretRef:
    name: example-cert
`

export function createEmptyEdgionTls(): EdgionTls {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'EdgionTls',
    metadata: { name: '', namespace: 'default' },
    spec: { hosts: [], secretRef: { name: '' } },
  }
}

export function normalizeEdgionTls(raw: unknown): EdgionTls {
  if (!raw || typeof raw !== 'object') throw new Error('EdgionTls must be an object')
  const resource = raw as EdgionTls
  if (resource.kind !== 'EdgionTls') throw new Error('Expected EdgionTls kind')
  if (!resource.metadata || !resource.spec) throw new Error('EdgionTls metadata and spec are required')
  return structuredClone(resource)
}

export function edgionTlsToYaml(tls: EdgionTls): string {
  return dumpYaml(tls)
}

export function yamlToEdgionTls(yamlStr: string): EdgionTls {
  return normalizeEdgionTls(yaml.load(yamlStr))
}

export function toMutationDocument(
  resource: EdgionTls,
  mode: 'create' | 'update',
): Record<string, unknown> {
  return buildMutationDocument(resource, { resourceKind: 'edgiontls', mode })
}
