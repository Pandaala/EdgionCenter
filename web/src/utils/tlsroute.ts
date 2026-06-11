/**
 * TLSRoute 工具函数
 */

import * as yaml from 'js-yaml'
import type { TLSRoute } from '@/types/gateway-api/tlsroute'
import { removeEmpty } from './yaml-utils'

export const DEFAULT_TLSROUTE_YAML = `apiVersion: gateway.networking.k8s.io/v1
kind: TLSRoute
metadata:
  name: example-tls-route
  namespace: default
spec:
  parentRefs:
    - name: example-gateway
      sectionName: tls-passthrough
  hostnames:
    - "secure.example.com"
  rules:
    - backendRefs:
        - name: example-service
          port: 8443
`

export function createEmptyTLSRoute(): TLSRoute {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'TLSRoute',
    metadata: {
      name: '',
      namespace: 'default',
    },
    spec: {
      parentRefs: [{ name: '', sectionName: '' }],
      hostnames: [],
      rules: [{ backendRefs: [{ name: '', port: 443 }] }],
    },
  }
}

export function normalizeTLSRoute(raw: any): TLSRoute {
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1',
    kind: 'TLSRoute',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: {
      parentRefs: raw.spec?.parentRefs || [],
      hostnames: raw.spec?.hostnames || [],
      rules: (raw.spec?.rules || []).map((rule: any) => ({
        backendRefs: rule.backendRefs || [],
      })),
    },
    status: raw.status,
  }
}


export function tlsRouteToYaml(route: TLSRoute): string {
  const clean = removeEmpty(route)
  return yaml.dump(clean, { lineWidth: -1, noRefs: true })
}

export function yamlToTLSRoute(yamlStr: string): TLSRoute {
  const raw = yaml.load(yamlStr) as any
  return normalizeTLSRoute(raw)
}
