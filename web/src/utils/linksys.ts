import * as yaml from 'js-yaml'
import type {
  ElasticsearchConfig,
  EtcdConfig,
  HttpDnsConfig,
  KafkaConfig,
  OtlpConfig,
  SecretObjectReference,
  LinkSys,
  LinkSysConfig,
  LinkSysType,
  RedisConfig,
  WebhookConfig,
} from '@/types/link-sys'
import { isValidDNS1123Label, isValidDNS1123Subdomain } from './validation'
import { dumpYaml } from './yaml-utils'
import { mutationDocumentToYaml } from './resource-document'

/** Operator fields derived from the Rust LinkSys structs; used as a drift/test matrix. */
export const LINKSYS_RUST_FIELD_MATRIX = {
  redis: ['endpoints','auth','db','timeout','pool','retry','topology','tls','observability'],
  elasticsearch: ['endpoints','auth','tls','timeout','pool','bulk','index'],
  etcd: ['endpoints','auth','tls','timeout','keepAlive','namespace','autoSyncInterval','maxCallSendSize','maxCallRecvSize','userAgent','rejectOldCluster','observability'],
  webhook: ['target','tls','timeoutMs','timeoutMsTemplate','retry','rateLimit','healthCheck','maxResponseBytes','success','statusOnError','request'],
  otlp: ['endpoint','timeoutMs','auth','tls'],
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
    case 'otlp':
      return { endpoint: '', timeoutMs: 10000 }
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

/** Match the Controller origin grammar before URL parsing can normalize input. */
export function isValidOtlpEndpoint(value: string): boolean {
  if (typeof value !== 'string' || value.length > 512 || /[^\x21-\x7e]|[\\%@?#]/.test(value)) return false
  const match = /^https?:\/\/([^/]+)\/?$/.exec(value)
  if (!match) return false
  const authority = match[1]
  const parts = authority.startsWith('[')
    ? /^\[([^\]]+)\](?::([0-9]+))?$/.exec(authority)
    : /^([^:]+)(?::([0-9]+))?$/.exec(authority)
  if (!parts) return false
  if (parts[2] !== undefined && (Number(parts[2]) < 1 || Number(parts[2]) > 65535)) return false
  if (authority.startsWith('[')) {
    try { return new URL(value).hostname.startsWith('[') } catch { return false }
  }
  const host = parts[1]
  if (/^[0-9.]+$/.test(host.replace(/\.$/, ''))) {
    return host.split('.').length === 4 && host.split('.').every((part) => /^(0|[1-9][0-9]{0,2})$/.test(part) && Number(part) <= 255)
  }
  const dns = host.replace(/\.$/, '')
  return dns.length <= 253 && dns.split('.').every((label) => label.length <= 63 && /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/i.test(label))
}

function validOtlpSecretRef(ref: SecretObjectReference | undefined): boolean {
  return !!ref && (ref.group == null || ref.group === '' || ref.group === 'core')
    && (ref.kind == null || ref.kind === 'Secret')
    && typeof ref.name === 'string' && ref.name.length <= 253 && isValidDNS1123Subdomain(ref.name)
    && (ref.namespace == null || (ref.namespace.length <= 63 && isValidDNS1123Label(ref.namespace)))
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
      if (webhook.timeoutMsTemplate && /\$\{(?:header|query|cookie|path|method|uri|secretRef):/i.test(webhook.timeoutMsTemplate)) {
        fail('timeoutMsTemplate must not read client-controlled or Secret variables')
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
    case 'otlp': {
      const otlp = config as OtlpConfig
      if (!isValidOtlpEndpoint(otlp.endpoint)) fail('OTLP endpoint must be an HTTP(S) origin without credentials, path, query, or fragment')
      const timeout = otlp.timeoutMs ?? 10000
      if (!Number.isInteger(timeout) || timeout < 1 || timeout > 300000) fail('OTLP timeoutMs must be an integer from 1 to 300000')
      if (otlp.auth && !validOtlpSecretRef(otlp.auth.secretRef)) fail('OTLP auth requires a valid core Secret reference')
      if (otlp.tls?.enabled) {
        if (!otlp.endpoint.startsWith('https://')) fail('OTLP HTTP endpoint cannot enable TLS policy')
        if (otlp.tls.verify === false) fail('OTLP TLS verification cannot be disabled')
        const validation = otlp.tls.validation
        if (validation?.hostname != null || validation?.subjectAltNames?.length) fail('OTLP TLS hostname and subjectAltNames overrides are unsupported')
        if (otlp.tls.clientCertificateRef && !validOtlpSecretRef(otlp.tls.clientCertificateRef)) fail('OTLP client certificate requires a valid core Secret reference')
        for (const ref of validation?.caCertificateRefs ?? []) {
          if (!validOtlpSecretRef(ref) || ref.kind !== 'Secret') fail('OTLP CA certificates require core Secret references')
        }
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
