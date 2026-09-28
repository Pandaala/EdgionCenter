/**
 * GRPCRoute 类型定义
 * apiVersion: gateway.networking.k8s.io/v1
 */

import type { ParentReference } from './backend'
import type { K8sObjectMeta, Hostname, Duration } from './common'

export type GRPCMethodMatchType = 'Exact' | 'RegularExpression'
export type GRPCHeaderMatchType = 'Exact' | 'RegularExpression'

export interface GRPCMethodMatch {
  type?: GRPCMethodMatchType
  service?: string
  method?: string
}

export interface GRPCHeaderMatch {
  type?: GRPCHeaderMatchType
  name: string
  value: string
}

export interface GRPCRouteMatch {
  method?: GRPCMethodMatch
  headers?: GRPCHeaderMatch[]
}

export const GRPC_ROUTE_FILTER_TYPES = ['RequestHeaderModifier', 'ResponseHeaderModifier', 'ExtensionRef'] as const

export interface GRPCRouteFilter {
  type: typeof GRPC_ROUTE_FILTER_TYPES[number]
  requestHeaderModifier?: import('./httproute').HTTPRequestHeaderFilter
  responseHeaderModifier?: import('./httproute').HTTPRequestHeaderFilter
  extensionRef?: import('./backend').LocalObjectReference
  [key: string]: unknown
}

export interface GRPCBackendRef {
  group?: string
  kind?: string
  namespace?: string
  name: string
  port?: number
  weight?: number
  filters?: GRPCRouteFilter[]
  [key: string]: unknown
}

export interface GRPCRouteRetry {
  attempts?: number
  backoff?: Duration
  codes?: number[]
  [key: string]: unknown
}

export interface GRPCSessionPersistence {
  sessionName?: string
  absoluteTimeout?: Duration
  type?: 'Cookie' | 'Header'
  cookieConfig?: { lifetimeType?: 'Permanent' | 'Session' }
  strict?: boolean
  [key: string]: unknown
}

export interface GRPCRouteTimeouts {
  request?: Duration
  backendRequest?: Duration
}

export interface GRPCRouteRule {
  name?: string
  matches?: GRPCRouteMatch[]
  filters?: GRPCRouteFilter[]
  backendRefs?: GRPCBackendRef[]
  timeouts?: GRPCRouteTimeouts
  retry?: GRPCRouteRetry
  sessionPersistence?: GRPCSessionPersistence
  [key: string]: unknown
}

export interface GRPCRouteSpec {
  parentRefs?: ParentReference[]
  hostnames?: Hostname[]
  rules?: GRPCRouteRule[]
  [key: string]: unknown
}

export interface GRPCRoute {
  apiVersion: string
  kind: 'GRPCRoute'
  metadata: K8sObjectMeta
  spec: GRPCRouteSpec
  status?: any
}
