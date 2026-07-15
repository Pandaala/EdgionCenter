import type { K8sObjectMeta } from '@/types/gateway-api/common'

export type LinkSysType = 'redis' | 'elasticsearch' | 'etcd' | 'webhook' | 'kafka' | 'httpdns'

export interface SecretObjectReference {
  name: string
  namespace?: string
}

export interface SecretAuth {
  secretRef: SecretObjectReference
}

export interface RedisConfig {
  endpoints: string[]
  db?: number
  auth?: SecretAuth
  topology?: {
    mode: 'standalone' | 'sentinel' | 'cluster'
    sentinel?: { masterName: string; sentinels: string[] }
    cluster?: { readFromReplicas?: boolean; maxRedirects?: number }
  }
}

export interface ElasticsearchConfig {
  endpoints: string[]
  auth?: SecretAuth & { type: 'basic' | 'apiKey' | 'bearer' }
}

export interface EtcdConfig {
  endpoints: string[]
  auth?: SecretAuth
}

export interface WebhookConfig {
  target: {
    url?: string
    blockPrivate?: boolean
    group?: string
    kind?: string
    name?: string
    namespace?: string
    port?: number
  }
  request?: {
    path?: unknown
    method?: { template: string }
    args?: unknown
    headers?: unknown
    cookies?: unknown
    body?: unknown
  }
  timeoutMs?: number
}

export interface KafkaConfig {
  brokers: string[]
  channelSize?: number
  lingerMs?: number
}

export interface HttpDnsConfig {
  preset?: 'aliyun' | 'tencent'
  urlTemplate?: string
  response?: {
    kind?: 'json' | 'delimited'
    ipPath?: string
    delimiter?: string
    ttlPath?: string
  }
  fallback?: { type: 'system' | 'none' | 'dns'; servers?: string[] }
  connection?: { timeoutMs?: number }
}

export type LinkSysConfig =
  | RedisConfig
  | ElasticsearchConfig
  | EtcdConfig
  | WebhookConfig
  | KafkaConfig
  | HttpDnsConfig

export interface LinkSysSpec {
  type: LinkSysType
  config: LinkSysConfig
}

export interface LinkSys {
  apiVersion: string
  kind: 'LinkSys'
  metadata: K8sObjectMeta
  spec: LinkSysSpec
  status?: any
}
