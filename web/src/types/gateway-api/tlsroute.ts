/**
 * TLSRoute 类型定义
 * apiVersion: gateway.networking.k8s.io/v1 (promoted from v1alpha3)
 */

import type { ParentReference, BackendRef } from './backend'
import type { K8sObjectMeta, Hostname } from './common'

export interface TLSRouteRule {
  backendRefs?: BackendRef[]
  [key: string]: unknown
}

export interface TLSRouteSpec {
  parentRefs?: ParentReference[]
  hostnames?: Hostname[]
  rules: TLSRouteRule[]
  [key: string]: unknown
}

export interface TLSRoute {
  apiVersion: string
  kind: 'TLSRoute'
  metadata: K8sObjectMeta
  spec: TLSRouteSpec
  status?: any
}
