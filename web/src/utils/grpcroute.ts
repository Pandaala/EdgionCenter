/**
 * GRPCRoute 工具函数
 */

import * as yaml from 'js-yaml'
import type { GRPCRoute, GRPCRouteRule } from '@/types/gateway-api/grpcroute'
import { removeEmpty } from './yaml-utils'

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

export function normalizeGRPCRoute(raw: any): GRPCRoute {
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1',
    kind: 'GRPCRoute',
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
      rules: (raw.spec?.rules || []).map((rule: any): GRPCRouteRule => ({
        name: rule.name,
        matches: rule.matches || [],
        filters: rule.filters || [],
        backendRefs: rule.backendRefs || [],
        timeouts: rule.timeouts,
        retry: rule.retry,
        sessionPersistence: rule.sessionPersistence,
      })),
    },
    status: raw.status,
  }
}


export function grpcRouteToYaml(route: GRPCRoute): string {
  const clean = removeEmpty(route)
  return yaml.dump(clean, { lineWidth: -1, noRefs: true })
}

export function yamlToGRPCRoute(yamlStr: string): GRPCRoute {
  const raw = yaml.load(yamlStr) as any
  return normalizeGRPCRoute(raw)
}
