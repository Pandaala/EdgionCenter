/**
 * TCPRoute 类型定义
 * apiVersion: gateway.networking.k8s.io/v1alpha2
 */

import type { ParentReference, BackendRef } from './backend'
import type { K8sObjectMeta } from './common'

export interface TCPRouteRule {
  backendRefs?: BackendRef[]
}

export interface TCPRouteSpec {
  parentRefs?: ParentReference[]
  rules: TCPRouteRule[]
}

export interface TCPRoute {
  apiVersion: string
  kind: 'TCPRoute'
  metadata: K8sObjectMeta
  spec: TCPRouteSpec
  status?: any
}
