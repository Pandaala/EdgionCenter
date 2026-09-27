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
  const sans = resource.spec.clientAuth?.allowedSans
  if (sans != null && (!Array.isArray(sans) || sans.some((entry) => !entry || typeof entry !== 'object' || Array.isArray(entry)))) throw new Error('clientAuth.allowedSans requires typed SAN objects')
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

export function validateEdgionTls(resource: EdgionTls): string[] {
  const errors: string[] = []
  if (resource.spec.hosts.length > 16) errors.push('spec.hosts must contain at most 16 entries')
  if ((resource.spec.parentRefs?.length ?? 0) > 32) errors.push('spec.parentRefs must contain at most 32 entries')
  const auth = resource.spec.clientAuth
  if (auth?.verifyDepth !== undefined && (!Number.isInteger(auth.verifyDepth) || auth.verifyDepth < 1 || auth.verifyDepth > 9)) errors.push('clientAuth.verifyDepth must be an integer from 1 to 9')
  auth?.allowedSans?.forEach((entry, index) => {
    const path = `clientAuth.allowedSans[${index}]`
    if (!entry || typeof entry !== 'object' || !['DNS', 'URI', 'Email', 'IP', 'OtherName'].includes(entry.type)) { errors.push(`${path} requires a typed SAN object`); return }
    if (typeof entry.value !== 'string' || !entry.value.trim()) errors.push(`${path}.value is required`)
    if (entry.match !== undefined && !['Exact', 'Prefix', 'Suffix', 'Contains', 'RegularExpression'].includes(entry.match)) errors.push(`${path}.match is invalid`)
    if (entry.type === 'OtherName') {
      if (!entry.oid || !/^(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*))+$/.test(entry.oid.trim())) errors.push(`${path}.oid must be a dotted-decimal object identifier`)
    } else if (entry.oid != null) errors.push(`${path}.oid is only valid for OtherName`)
    if (entry.match === 'RegularExpression' && typeof entry.value === 'string' && new TextEncoder().encode(entry.value.trim()).length > 1024) errors.push(`${path}.value exceeds the 1024-byte regex limit`)
  })
  return errors
}
