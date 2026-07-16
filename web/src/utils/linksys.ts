import * as yaml from 'js-yaml'
import type {
  ElasticsearchConfig,
  EtcdConfig,
  HttpDnsConfig,
  KafkaConfig,
  LinkSys,
  LinkSysConfig,
  LinkSysType,
  RedisConfig,
  WebhookConfig,
} from '@/types/link-sys'
import { dumpYaml } from './yaml-utils'
import { mutationDocumentToYaml } from './resource-document'

/** Operator fields derived from the Rust LinkSys structs; used as a drift/test matrix. */
export const LINKSYS_RUST_FIELD_MATRIX = {
  redis: ['endpoints','auth','db','timeout','pool','retry','topology','tls','observability'],
  elasticsearch: ['endpoints','auth','tls','timeout','pool','bulk','index'],
  etcd: ['endpoints','auth','tls','timeout','keepAlive','namespace','autoSyncInterval','maxCallSendSize','maxCallRecvSize','userAgent','rejectOldCluster','observability'],
  webhook: ['target','tls','timeoutMs','timeoutMsTemplate','retry','rateLimit','healthCheck','maxResponseBytes','success','allowDegradation','statusOnError','allowDegradationTemplate','request'],
  kafka: ['brokers','sasl','tls','channelSize','lingerMs'],
  httpdns: ['preset','urlTemplate','response','fallback','connection'],
} as const

export const DEFAULT_YAML = `apiVersion: edgion.io/v1
kind: LinkSys
metadata:
  name: redis-cluster
  namespace: default
spec:
  type: redis
  config:
    endpoints:
      - redis://127.0.0.1:6379
    db: 0
    topology:
      mode: standalone
`

export function createConfig(type: LinkSysType): LinkSysConfig {
  switch (type) {
    case 'redis':
      return { endpoints: [], db: 0, topology: { mode: 'standalone' } }
    case 'elasticsearch':
      return { endpoints: [] }
    case 'etcd':
      return { endpoints: [] }
    case 'webhook':
      return { target: { url: '' }, request: { method: { template: 'POST' } }, timeoutMs: 5000 }
    case 'kafka':
      return { brokers: [] }
    case 'httpdns':
      return { urlTemplate: '', response: { kind: 'json', ipPath: 'ips' }, fallback: { type: 'system' } }
  }
}

export function createEmpty(): LinkSys {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'LinkSys',
    metadata: { name: '', namespace: 'default' },
    spec: { type: 'redis', config: createConfig('redis') },
  }
}

export function normalize(raw: unknown): LinkSys {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('LinkSys document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'LinkSys') throw new Error('Expected a LinkSys document')
  return structuredClone(document) as unknown as LinkSys
}

export function redisConfig(resource: LinkSys): RedisConfig {
  return resource.spec.config as RedisConfig
}

export function elasticsearchConfig(resource: LinkSys): ElasticsearchConfig {
  return resource.spec.config as ElasticsearchConfig
}

export function etcdConfig(resource: LinkSys): EtcdConfig {
  return resource.spec.config as EtcdConfig
}

export function webhookConfig(resource: LinkSys): WebhookConfig {
  return resource.spec.config as WebhookConfig
}

export function kafkaConfig(resource: LinkSys): KafkaConfig {
  return resource.spec.config as KafkaConfig
}

export function httpDnsConfig(resource: LinkSys): HttpDnsConfig {
  return resource.spec.config as HttpDnsConfig
}

export function withWebhookUrl(config: WebhookConfig, url: string): WebhookConfig {
  if (!url) return { ...config, target: { ...config.target, url: undefined } }
  return { ...config, target: { url, blockPrivate: config.target.blockPrivate } }
}

export function withWebhookServiceTarget(config: WebhookConfig, partial: Partial<WebhookConfig['target']>): WebhookConfig {
  const target = { ...config.target, url: undefined, blockPrivate: undefined, ...partial }
  return { ...config, target }
}

export function withWebhookMethod(config: WebhookConfig, template: string): WebhookConfig {
  return {
    ...config,
    request: { ...config.request, method: { template } },
  }
}

export function validateLinkSys(resource: LinkSys): void {
  const fail = (message: string): never => { throw new Error(message) }
  const config = resource.spec.config

  switch (resource.spec.type) {
    case 'redis': {
      const redis = config as RedisConfig
      if (!redis.endpoints?.length) fail('Redis requires at least one endpoint')
      if (redis.auth && !redis.auth.secretRef?.name) fail('Redis auth requires a Secret name')
      if (redis.topology?.mode === 'sentinel' && (
        !redis.topology.sentinel?.masterName || !redis.topology.sentinel.sentinels?.length
      )) fail('Sentinel mode requires a master name and at least one endpoint')
      if (redis.topology?.mode === 'cluster' && !redis.topology.cluster) fail('Cluster mode requires cluster settings')
      break
    }
    case 'elasticsearch': {
      const elasticsearch = config as ElasticsearchConfig
      if (!elasticsearch.endpoints?.length) fail('Elasticsearch requires at least one endpoint')
      if (elasticsearch.auth && !elasticsearch.auth.secretRef?.name) fail('Elasticsearch auth requires a Secret name')
      break
    }
    case 'etcd': {
      const etcd = config as EtcdConfig
      if (!etcd.endpoints?.length) fail('etcd requires at least one endpoint')
      if (etcd.auth && !etcd.auth.secretRef?.name) fail('etcd auth requires a Secret name')
      break
    }
    case 'webhook': {
      const webhook = config as WebhookConfig
      const hasUrl = Boolean(webhook.target?.url)
      const hasService = Boolean(webhook.target?.group || webhook.target?.kind || webhook.target?.name || webhook.target?.namespace || webhook.target?.port)
      if (hasUrl === hasService) fail('Webhook requires exactly one URL or Service target')
      if (hasService && !webhook.target.port) fail('Webhook Service target requires a port')
      if (hasService && webhook.target.blockPrivate !== undefined) fail('Webhook blockPrivate applies to URL targets only')
      if (hasUrl) {
        let parsed: URL
        try { parsed = new URL(webhook.target.url!) } catch { fail('Webhook target URL is invalid') }
        if (!['http:', 'https:'].includes(parsed!.protocol) || parsed!.username || parsed!.password || (parsed!.pathname && parsed!.pathname !== '/') || parsed!.search || parsed!.hash) fail('Webhook target URL must be an http(s) origin without credentials, path, query, or fragment')
        if (parsed!.protocol === 'http:' && webhook.tls?.enabled) fail('Webhook http target cannot enable TLS')
      }
      if ((webhook.timeoutMs ?? 5000) < 1 || (webhook.timeoutMs ?? 5000) > 60_000) fail('Webhook timeoutMs must be between 1 and 60000')
      if (webhook.maxResponseBytes !== undefined && webhook.maxResponseBytes < 1) fail('Webhook maxResponseBytes must be greater than 0')
      if (webhook.statusOnError !== undefined && (webhook.statusOnError < 200 || webhook.statusOnError > 599)) fail('Webhook statusOnError must be between 200 and 599')
      for (const [name, template] of [['timeoutMsTemplate',webhook.timeoutMsTemplate],['allowDegradationTemplate',webhook.allowDegradationTemplate]] as const) {
        if (template && /\$\{(?:header|query|cookie|path|method|uri|secretRef):/i.test(template)) fail(`${name} must not read client-controlled or Secret variables`)
      }
      const retry = webhook.retry as any
      if (retry?.maxRetries > 10) fail('Webhook retry.maxRetries must be between 0 and 10')
      if (retry?.retryDelayMs !== undefined && retry.retryDelayMs < 1) fail('Webhook retry.retryDelayMs must be greater than 0')
      if (retry?.maxDelayMs !== undefined && (retry.maxDelayMs < 1 || retry.maxDelayMs > 10_000)) fail('Webhook retry.maxDelayMs must be between 1 and 10000')
      const rate = webhook.rateLimit as any
      if (rate && (!(rate.rate > 0) || !(rate.windowSec > 0))) fail('Webhook rateLimit rate and windowSec must be greater than 0')
      const request = webhook.request as any
      if (request?.args?.forwardAll || request?.cookies?.forwardAll) fail('Webhook forwardAll is only supported for headers')
      const success = webhook.success as any
      if (success?.statusCodes && !success.statusCodes.length) fail('Webhook success.statusCodes must not be empty')
      for (const predicate of success?.body ?? []) {
        if (!predicate.pointer) fail('Webhook success body predicate pointer is required')
        const count = ['equals','notEquals','exists','in'].filter((key) => predicate[key] !== undefined).length
        if (count !== 1) fail('Webhook success body predicate requires exactly one operator')
      }
      break
    }
    case 'kafka':
      if (!(config as KafkaConfig).brokers?.length) fail('Kafka requires at least one broker')
      break
    case 'httpdns': {
      const httpDns = config as HttpDnsConfig
      if (!httpDns.preset && !httpDns.urlTemplate) fail('HTTP DNS requires a preset or URL template')
      if (httpDns.urlTemplate && !httpDns.urlTemplate.includes('{domain}')) {
        fail('HTTP DNS URL template must contain {domain}')
      }
      break
    }
  }
}

export function toYaml(ls: LinkSys): string {
  return dumpYaml(ls)
}

export function toMutationYaml(ls: LinkSys, mode: 'create' | 'update'): string {
  return mutationDocumentToYaml(ls, 'linksys', mode)
}

export function fromYaml(yamlStr: string): LinkSys {
  return normalize(yaml.load(yamlStr) as any)
}
