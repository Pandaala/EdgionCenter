/**
 * Gateway 类型定义
 * apiVersion: gateway.networking.k8s.io/v1
 */

import type { K8sObjectMeta, Hostname } from './common'

export type ListenerProtocol = 'HTTP' | 'HTTPS' | 'TCP' | 'TLS' | 'UDP'
export type TLSMode = 'Terminate' | 'Passthrough'
export type NamespacesFromType = 'Same' | 'All' | 'Selector'

export interface CertificateRef {
  name: string
  namespace?: string
  kind?: string
  group?: string
}

export interface ListenerTLS {
  mode?: TLSMode
  certificateRefs?: CertificateRef[]
  options?: Record<string, string>
}

export interface AllowedRoutes {
  namespaces?: {
    from?: NamespacesFromType
    selector?: { matchLabels?: Record<string, string> }
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
}

export interface GatewayAddress {
  type?: 'IPAddress' | 'Hostname'
  value: string
}

export interface GatewaySpec {
  gatewayClassName: string
  listeners: GatewayListener[]
  addresses?: GatewayAddress[]
}

export interface Gateway {
  apiVersion: string
  kind: 'Gateway'
  metadata: K8sObjectMeta
  spec: GatewaySpec
  status?: any
}
