/**
 * EdgionTls 工具函数
 */

import * as yaml from 'js-yaml'
import type { EdgionTls } from '@/types/edgion-tls'
import { dumpYaml } from './yaml-utils'

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

export function normalizeEdgionTls(raw: any): EdgionTls {
  return {
    apiVersion: raw.apiVersion || 'edgion.io/v1',
    kind: 'EdgionTls',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: {
      parentRefs: raw.spec?.parentRefs,
      hosts: raw.spec?.hosts || [],
      secretRef: raw.spec?.secretRef || { name: '' },
      clientAuth: raw.spec?.clientAuth,
      minTlsVersion: raw.spec?.minTlsVersion,
      cipherSuites: raw.spec?.cipherSuites,
    },
    status: raw.status,
  }
}

export function edgionTlsToYaml(tls: EdgionTls): string {
  return dumpYaml(tls)
}

export function yamlToEdgionTls(yamlStr: string): EdgionTls {
  return normalizeEdgionTls(yaml.load(yamlStr) as any)
}
