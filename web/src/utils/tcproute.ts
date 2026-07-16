/**
 * TCPRoute 工具函数
 */

import * as yaml from 'js-yaml'
import type { TCPRoute } from '@/types/gateway-api/tcproute'
import { mutationDocumentToYaml } from './resource-document'

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

export function normalizeTCPRoute(raw: unknown): TCPRoute {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('TCPRoute document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'TCPRoute') throw new Error('Expected a TCPRoute document')
  return structuredClone(document) as unknown as TCPRoute
}


export function tcpRouteToYaml(route: TCPRoute): string {
  return yaml.dump(route, { lineWidth: -1, noRefs: true })
}

export function tcpRouteToMutationYaml(route: TCPRoute, mode: 'create' | 'update'): string {
  return mutationDocumentToYaml(route, 'tcproute', mode)
}

export function yamlToTCPRoute(yamlStr: string): TCPRoute {
  const raw = yaml.load(yamlStr) as any
  return normalizeTCPRoute(raw)
}
