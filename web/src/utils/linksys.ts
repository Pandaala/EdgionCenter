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

export function normalize(raw: any): LinkSys {
  const type = (raw.spec?.type || 'redis') as LinkSysType
  return {
    apiVersion: raw.apiVersion || 'edgion.io/v1',
    kind: 'LinkSys',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: {
      type,
      config: raw.spec?.config || createConfig(type),
    },
    status: raw.status,
  }
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
  return { ...config, target: { ...config.target, url } }
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
      const hasService = Boolean(webhook.target?.name)
      if (hasUrl === hasService) fail('Webhook requires exactly one URL or Service target')
      if (hasService && !webhook.target.port) fail('Webhook Service target requires a port')
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

export function fromYaml(yamlStr: string): LinkSys {
  return normalize(yaml.load(yamlStr) as any)
}
