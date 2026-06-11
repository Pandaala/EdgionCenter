/**
 * TCPRoute 工具函数
 */

import * as yaml from 'js-yaml'
import type { TCPRoute } from '@/types/gateway-api/tcproute'
import { removeEmpty } from './yaml-utils'

export const DEFAULT_TCPROUTE_YAML = `apiVersion: gateway.networking.k8s.io/v1alpha2
kind: TCPRoute
metadata:
  name: example-tcp-route
  namespace: default
spec:
  parentRefs:
    - name: example-gateway
      sectionName: tcp-9000
  rules:
    - backendRefs:
        - name: example-service
          port: 9000
`

export function createEmptyTCPRoute(): TCPRoute {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1alpha2',
    kind: 'TCPRoute',
    metadata: {
      name: '',
      namespace: 'default',
    },
    spec: {
      parentRefs: [{ name: '', sectionName: '' }],
      rules: [{ backendRefs: [{ name: '', port: 80 }] }],
    },
  }
}

export function normalizeTCPRoute(raw: any): TCPRoute {
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1alpha2',
    kind: 'TCPRoute',
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
      rules: (raw.spec?.rules || []).map((rule: any) => ({
        backendRefs: rule.backendRefs || [],
      })),
    },
    status: raw.status,
  }
}


export function tcpRouteToYaml(route: TCPRoute): string {
  const clean = removeEmpty(route)
  return yaml.dump(clean, { lineWidth: -1, noRefs: true })
}

export function yamlToTCPRoute(yamlStr: string): TCPRoute {
  const raw = yaml.load(yamlStr) as any
  return normalizeTCPRoute(raw)
}
