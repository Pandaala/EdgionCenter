/**
 * EdgionStreamPlugins 类型定义
 * apiVersion: edgion.io/v1
 */

import type { K8sObjectMeta } from '@/types/gateway-api/common'

export interface IpRestrictionConfig {
  ipSource?: string
  allow?: string[]
  deny?: string[]
  defaultAction?: 'allow' | 'deny'
  message?: string
}

export interface StreamPlugin {
  type: string
  config?: IpRestrictionConfig | Record<string, any>
}

export interface EdgionStreamPluginsSpec {
  plugins: StreamPlugin[]
}

export interface EdgionStreamPlugins {
  apiVersion: string
  kind: 'EdgionStreamPlugins'
  metadata: K8sObjectMeta
  spec: EdgionStreamPluginsSpec
  status?: any
}
