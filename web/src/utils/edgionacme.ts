import * as yaml from 'js-yaml'
import type { EdgionAcme } from '@/types/edgion-acme'
import { dumpYaml } from './yaml-utils'

export const DEFAULT_YAML = `apiVersion: edgion.io/v1
kind: EdgionAcme
metadata:
  name: lets-encrypt
  namespace: default
spec:
  email: "admin@example.com"
  domains:
    - "example.com"
  challenge:
    type: http-01
    http01:
      gatewayRef:
        name: my-gateway
  storage:
    secretName: acme-cert
  renewal:
    renewBeforeDays: 30
  autoEdgionTls:
    enabled: true
`

export function createEmpty(): EdgionAcme {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'EdgionAcme',
    metadata: { name: '', namespace: 'default' },
    spec: {
      email: '',
      domains: [],
      challenge: { type: 'http-01', http01: { gatewayRef: { name: '' } } },
      storage: { secretName: '' },
      renewal: { renewBeforeDays: 30 },
      autoEdgionTls: { enabled: true },
    },
  }
}

export function normalize(raw: any): EdgionAcme {
  return {
    apiVersion: raw.apiVersion || 'edgion.io/v1',
    kind: 'EdgionAcme',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: raw.spec || { email: '', domains: [], challenge: { type: 'http-01' }, storage: { secretName: '' } },
    status: raw.status,
  }
}

export function toYaml(acme: EdgionAcme): string {
  return dumpYaml(acme)
}

export function fromYaml(yamlStr: string): EdgionAcme {
  return normalize(yaml.load(yamlStr) as any)
}
