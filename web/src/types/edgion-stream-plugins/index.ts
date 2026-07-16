/**
 * EdgionStreamPlugins 类型定义
 * apiVersion: edgion.io/v1
 */

import type { K8sObjectMeta } from '@/types/gateway-api/common'

export interface IpGroup {
  name: string
  description?: string
  cidrs: string[]
  [key: string]: unknown
}

export interface ConfigDataRef {
  name: string
  namespace?: string
  [key: string]: unknown
}

/** Stage-1 L4 IpRestriction wire shape. HTTP-only fields are intentionally absent. */
export interface ConnectionIpRestrictionConfig {
  allow?: IpGroup[]
  deny?: IpGroup[]
  defaultAction?: 'allow' | 'deny'
  allowRefs?: ConfigDataRef[]
  denyRefs?: ConfigDataRef[]
  [key: string]: unknown
}

/** Stage-2 uses the HTTP IpRestriction shape because TLS routing has richer context. */
export interface TlsRouteIpRestrictionConfig extends ConnectionIpRestrictionConfig {
  ipSource?: 'clientIp' | 'remoteAddr'
  message?: string
  status?: number
}

export interface StreamPlugin {
  enable?: boolean
  type: 'IpRestriction' | 'GlobalConnectionIpRestriction' | 'ConnectionRateLimit' | string
  config?: ConnectionIpRestrictionConfig | Record<string, unknown>
  [key: string]: unknown
}

export interface EdgionStreamPluginsSpec {
  plugins?: StreamPlugin[]
  tlsRoutePlugins?: StreamPlugin[]
  [key: string]: unknown
}

export interface EdgionStreamPlugins {
  apiVersion: string
  kind: 'EdgionStreamPlugins'
  metadata: K8sObjectMeta
  spec: EdgionStreamPluginsSpec
  status?: any
  [key: string]: unknown
}
