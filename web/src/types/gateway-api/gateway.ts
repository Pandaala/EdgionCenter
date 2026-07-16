/**
 * Gateway 类型定义
 * apiVersion: gateway.networking.k8s.io/v1
 */

import type { K8sObjectMeta, Hostname } from './common'

export type ListenerProtocol = string
export type TLSMode = 'Terminate' | 'Passthrough'
export type NamespacesFromType = 'Same' | 'All' | 'Selector'

export interface CertificateRef {
  name: string
  namespace?: string
  kind?: string
  group?: string
  [key: string]: unknown
}

export interface FrontendTLSValidation {
  mode?: 'AllowValidOnly' | 'AllowInsecureFallback'
  caCertificateRefs?: CertificateRef[]
  [key: string]: unknown
}

export interface GatewayFrontendTLSDefault {
  validation?: FrontendTLSValidation
  [key: string]: unknown
}

export interface GatewayFrontendTLSPerPort {
  port: number
  tls?: GatewayFrontendTLSDefault
  [key: string]: unknown
}

export interface GatewaySpecTLS {
  backend?: {
    clientCertificateRef?: CertificateRef
    [key: string]: unknown
  }
  frontend?: {
    default?: GatewayFrontendTLSDefault
    perPort?: GatewayFrontendTLSPerPort[]
    [key: string]: unknown
  }
  [key: string]: unknown
}

export interface ListenerTLS {
  mode?: TLSMode
  certificateRefs?: CertificateRef[]
  options?: Record<string, unknown>
  frontendValidation?: FrontendTLSValidation
  [key: string]: unknown
}

export interface AllowedRoutes {
  namespaces?: {
    from?: NamespacesFromType
    selector?: {
      matchLabels?: Record<string, string>
      matchExpressions?: Array<{
        key: string
        operator: 'In' | 'NotIn' | 'Exists' | 'DoesNotExist'
        values?: string[]
        [key: string]: unknown
      }>
      [key: string]: unknown
    }
  }
  kinds?: Array<{ group?: string; kind: string }>
}

export interface GatewayListener {
  name: string
  port: number
  protocol: ListenerProtocol
  hostname?: Hostname
  tls?: ListenerTLS
  allowedRoutes?: AllowedRoutes
  [key: string]: unknown
}

export interface GatewayAddress {
  type?: string
  value: string
  [key: string]: unknown
}

export interface GatewaySpec {
  gatewayClassName: string
  listeners: GatewayListener[]
  addresses?: GatewayAddress[]
  tls?: GatewaySpecTLS
  [key: string]: unknown
}

export interface Gateway {
  apiVersion: string
  kind: 'Gateway'
  metadata: K8sObjectMeta
  spec: GatewaySpec
  status?: any
  [key: string]: unknown
}
