/**
 * LinkSys 类型定义
 * apiVersion: edgion.io/v1
 */

import type { K8sObjectMeta } from '@/types/gateway-api/common'

export type LinkSysType = 'redis' | 'elasticsearch' | 'etcd' | 'webhook'

export interface RedisConfig {
  addresses?: string[]
  password?: string
  database?: number
  clusterMode?: boolean
  tls?: { enable?: boolean }
}

export interface ElasticsearchConfig {
  addresses?: string[]
  username?: string
  password?: string
}

export interface EtcdConfig {
  endpoints?: string[]
  username?: string
  password?: string
}

export interface WebhookConfig {
  url?: string
  method?: string
  headers?: Record<string, string>
  timeoutMs?: number
}

export interface LinkSysSpec {
  type: LinkSysType
  redis?: RedisConfig
  elasticsearch?: ElasticsearchConfig
  etcd?: EtcdConfig
  webhook?: WebhookConfig
}

export interface LinkSys {
  apiVersion: string
  kind: 'LinkSys'
  metadata: K8sObjectMeta
  spec: LinkSysSpec
  status?: any
}
