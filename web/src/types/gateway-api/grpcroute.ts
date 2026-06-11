/**
 * GRPCRoute 类型定义
 * apiVersion: gateway.networking.k8s.io/v1
 */

import type { ParentReference, BackendRef } from './backend'
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

export interface GRPCRouteFilter {
  type: string
  requestHeaderModifier?: any
  responseHeaderModifier?: any
  requestMirror?: any
  extensionRef?: any
}

export interface GRPCRouteRetry {
  attempts?: number
  backoff?: Duration
  codes?: number[]
}

export interface GRPCSessionPersistence {
  sessionName?: string
  absoluteTimeout?: Duration
  idleTimeout?: Duration
  type?: 'Cookie' | 'Header'
  cookieConfig?: { lifetimeType?: 'Permanent' | 'Session' }
}

export interface GRPCRouteTimeouts {
  request?: Duration
  backendRequest?: Duration
}

export interface GRPCRouteRule {
  name?: string
  matches?: GRPCRouteMatch[]
  filters?: GRPCRouteFilter[]
  backendRefs?: BackendRef[]
  timeouts?: GRPCRouteTimeouts
  retry?: GRPCRouteRetry
  sessionPersistence?: GRPCSessionPersistence
}

export interface GRPCRouteSpec {
  parentRefs?: ParentReference[]
  hostnames?: Hostname[]
  rules?: GRPCRouteRule[]
}

export interface GRPCRoute {
  apiVersion: string
  kind: 'GRPCRoute'
  metadata: K8sObjectMeta
  spec: GRPCRouteSpec
  status?: any
}
