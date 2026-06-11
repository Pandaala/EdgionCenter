/**
 * UDPRoute 类型定义
 * apiVersion: gateway.networking.k8s.io/v1alpha2
 */

import type { ParentReference, BackendRef } from './backend'
import type { K8sObjectMeta } from './common'

export interface UDPRouteRule {
  backendRefs?: BackendRef[]
}

export interface UDPRouteSpec {
  parentRefs?: ParentReference[]
  rules: UDPRouteRule[]
}

export interface UDPRoute {
  apiVersion: string
  kind: 'UDPRoute'
  metadata: K8sObjectMeta
  spec: UDPRouteSpec
  status?: any
}
