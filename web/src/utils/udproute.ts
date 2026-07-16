/**
 * UDPRoute 工具函数
 */

import * as yaml from 'js-yaml'
import type { UDPRoute } from '@/types/gateway-api/udproute'
import { mutationDocumentToYaml } from './resource-document'

export const DEFAULT_UDPROUTE_YAML = `apiVersion: gateway.networking.k8s.io/v1alpha2
kind: UDPRoute
metadata:
  name: example-udp-route
  namespace: default
spec:
  parentRefs:
    - name: example-gateway
      sectionName: udp-5300
  rules:
    - backendRefs:
        - name: example-service
          port: 5300
`

export function createEmptyUDPRoute(): UDPRoute {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1alpha2',
    kind: 'UDPRoute',
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

export function normalizeUDPRoute(raw: unknown): UDPRoute {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('UDPRoute document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'UDPRoute') throw new Error('Expected a UDPRoute document')
  return structuredClone(document) as unknown as UDPRoute
}


export function udpRouteToYaml(route: UDPRoute): string {
  return yaml.dump(route, { lineWidth: -1, noRefs: true })
}

export function udpRouteToMutationYaml(route: UDPRoute, mode: 'create' | 'update'): string {
  return mutationDocumentToYaml(route, 'udproute', mode)
}

export function yamlToUDPRoute(yamlStr: string): UDPRoute {
  const raw = yaml.load(yamlStr) as any
  return normalizeUDPRoute(raw)
}
