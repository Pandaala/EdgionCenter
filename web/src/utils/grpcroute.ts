/**
 * GRPCRoute 工具函数
 */

import * as yaml from 'js-yaml'
import type { GRPCRoute } from '@/types/gateway-api/grpcroute'
import { mutationDocumentToYaml } from './resource-document'

export const DEFAULT_GRPCROUTE_YAML = `apiVersion: gateway.networking.k8s.io/v1
kind: GRPCRoute
metadata:
  name: example-grpc-route
  namespace: default
spec:
  parentRefs:
    - name: example-gateway
      sectionName: grpc-https
  hostnames:
    - "grpc.example.com"
  rules:
    - matches:
        - method:
            type: Exact
            service: "mypackage.MyService"
            method: "GetItem"
      backendRefs:
        - name: grpc-service
          port: 50051
`

export function createEmptyGRPCRoute(): GRPCRoute {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'GRPCRoute',
    metadata: {
      name: '',
      namespace: 'default',
    },
    spec: {
      parentRefs: [{ name: '', sectionName: '' }],
      hostnames: [],
      rules: [
        {
          matches: [{ method: { type: 'Exact', service: '', method: '' } }],
          backendRefs: [{ name: '', port: 50051 }],
        },
      ],
    },
  }
}

export function normalizeGRPCRoute(raw: unknown): GRPCRoute {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('GRPCRoute document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'GRPCRoute') throw new Error('Expected a GRPCRoute document')
  return structuredClone(document) as unknown as GRPCRoute
}


export function grpcRouteToYaml(route: GRPCRoute): string {
  return yaml.dump(route, { lineWidth: -1, noRefs: true })
}

export function grpcRouteToMutationYaml(route: GRPCRoute, mode: 'create' | 'update'): string {
  validateGRPCRouteForMutation(route)
  return mutationDocumentToYaml(route, 'grpcroute', mode)
}

const isGRPCDelegationRef = (ref: any) =>
  ref?.group === 'gateway.networking.k8s.io' && ref?.kind === 'GRPCRoute'

export function validateGRPCRouteForMutation(route: GRPCRoute): void {
  for (const [ruleIndex, rule] of (route.spec.rules || []).entries()) {
    for (const code of rule.retry?.codes || []) {
      if (!Number.isInteger(code) || code < 0 || code > 16) {
        throw new Error(`rules[${ruleIndex}].retry.codes must contain gRPC status codes from 0 through 16`)
      }
    }
    if ((rule.backendRefs || []).some(isGRPCDelegationRef)) {
      if (rule.backendRefs?.length !== 1) throw new Error(`rules[${ruleIndex}] delegation requires exactly one backendRef`)
      const ref = rule.backendRefs[0]
      if (ref.port !== undefined) throw new Error(`rules[${ruleIndex}] delegation backendRef must not set port`)
      if (ref.filters !== undefined) throw new Error(`rules[${ruleIndex}] delegation backendRef must not set filters`)
      if (!rule.matches?.length) throw new Error(`rules[${ruleIndex}] gRPC delegation requires at least one match`)
      for (const match of rule.matches) {
        if (!match.method?.service) throw new Error(`rules[${ruleIndex}] gRPC delegation matches require method.service`)
        if (match.method.type && match.method.type !== 'Exact') throw new Error(`rules[${ruleIndex}] gRPC delegation matches must use Exact`)
        if (match.method.method !== undefined) throw new Error(`rules[${ruleIndex}] gRPC delegation matches must not set method.method`)
      }
    }
  }
}

export function yamlToGRPCRoute(yamlStr: string): GRPCRoute {
  const raw = yaml.load(yamlStr) as any
  return normalizeGRPCRoute(raw)
}
