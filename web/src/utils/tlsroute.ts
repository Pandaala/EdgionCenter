/**
 * TLSRoute 工具函数
 */

import * as yaml from 'js-yaml'
import type { TLSRoute } from '@/types/gateway-api/tlsroute'
import { mutationDocumentToYaml } from './resource-document'

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

export function normalizeTLSRoute(raw: unknown): TLSRoute {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('TLSRoute document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'TLSRoute') throw new Error('Expected a TLSRoute document')
  return structuredClone(document) as unknown as TLSRoute
}


export function tlsRouteToYaml(route: TLSRoute): string {
  return yaml.dump(route, { lineWidth: -1, noRefs: true })
}

export function tlsRouteToMutationYaml(route: TLSRoute, mode: 'create' | 'update'): string {
  return mutationDocumentToYaml(route, 'tlsroute', mode)
}

export function yamlToTLSRoute(yamlStr: string): TLSRoute {
  const raw = yaml.load(yamlStr) as any
  return normalizeTLSRoute(raw)
}
