import type { K8sObjectMeta } from '@/types/gateway-api/common'

export type LinkSysType = 'redis' | 'elasticsearch' | 'etcd' | 'webhook' | 'kafka' | 'httpdns' | 'otlp' | 'credentialSource'

export interface SecretObjectReference {
  group?: string
  kind?: string
  name: string
  namespace?: string
}

export interface SecretAuth {
  secretRef: SecretObjectReference
}

export interface LinkTlsConfig {
  enabled?: boolean
  verify?: boolean
  validation?: { caCertificateRefs?: Array<SecretObjectReference>; wellKnownCACertificates?: 'System'; hostname?: string; subjectAltNames?: Array<{ type: 'Hostname' | 'URI'; hostname?: string; uri?: string }> }
  clientCertificateRef?: SecretObjectReference
}

export interface LinkObservability {
  metrics?: { enabled?: boolean; labels?: Record<string, string> }
  logging?: { enabled?: boolean; level?: string; slowLogThreshold?: number }
}

export interface RedisConfig {
  endpoints: string[]
  db?: number
  auth?: SecretAuth
  timeout?: { connect?: number; read?: number; write?: number }
  pool?: { size?: number; minIdle?: number }
  retry?: { maxRetries?: number; backoff?: { type?: string; initialDelay?: number; maxDelay?: number; multiplier?: number } }
  topology?: {
    mode: 'standalone' | 'sentinel' | 'cluster'
    sentinel?: { masterName: string; sentinels: string[] }
    cluster?: { readFromReplicas?: boolean; maxRedirects?: number }
  }
  tls?: LinkTlsConfig
  observability?: LinkObservability
}

export interface ElasticsearchConfig {
  endpoints: string[]
  auth?: SecretAuth & { type: 'basic' | 'apiKey' | 'bearer' }
  tls?: LinkTlsConfig
  timeout?: { connect?: number; request?: number }
  pool?: { maxIdlePerHost?: number; idleTimeout?: number }
  bulk?: { batchSize?: number; flushInterval?: number; maxRetries?: number; backoffMs?: number; maxBodyBytes?: number; channelSize?: number }
  index?: { prefix?: string; datePattern?: string }
}

export interface EtcdConfig {
  endpoints: string[]
  auth?: SecretAuth
  tls?: LinkTlsConfig
  timeout?: { dial?: number; request?: number; keepAlive?: number }
  keepAlive?: { time?: number; timeout?: number; permitWithoutStream?: boolean }
  namespace?: string
  autoSyncInterval?: number
  maxCallSendSize?: number
  maxCallRecvSize?: number
  userAgent?: string
  rejectOldCluster?: boolean
  observability?: LinkObservability
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
  tls?: LinkTlsConfig
  timeoutMsTemplate?: string
  retry?: Record<string, unknown>
  rateLimit?: Record<string, unknown>
  healthCheck?: Record<string, unknown>
  maxResponseBytes?: number
  success?: Record<string, unknown>
  statusOnError?: number
}

export interface KafkaConfig {
  brokers: string[]
  sasl?: { username?: string; password?: SecretAuth }
  tls?: LinkTlsConfig
  channelSize?: number
  lingerMs?: number
}

export interface OtlpConfig {
  endpoint: string
  timeoutMs?: number
  auth?: SecretAuth
  tls?: LinkTlsConfig
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
  connection?: { timeoutMs?: number; tls?: LinkTlsConfig }
}

export interface CredentialSourceConfig {
  provider: {
    type: 'oauth2ClientCredentials'
    tokenEndpoint: string
    clientAuthentication: {
      method?: 'clientSecretBasic'
      activeSecretRef: SecretObjectReference
      previousSecretRef?: SecretObjectReference
    }
    scopes?: string[]
    tls?: LinkTlsConfig
  }
  rotation?: {
    interval?: string
    requestTimeout?: string
    retryInitialBackoff?: string
    retryMaxBackoff?: string
  }
  egress?: { blockPrivate?: boolean }
  publication?: { persist?: boolean; memoryMaxKeys?: number }
}

export type LinkSysConfig =
  | RedisConfig
  | ElasticsearchConfig
  | EtcdConfig
  | WebhookConfig
  | KafkaConfig
  | HttpDnsConfig
  | OtlpConfig
  | CredentialSourceConfig

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
