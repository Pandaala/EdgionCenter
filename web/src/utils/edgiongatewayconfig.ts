import * as yaml from 'js-yaml'
import type { EdgionGatewayConfig } from '@/types/edgion-gateway-config'
import { dumpYaml } from './yaml-utils'
import { mutationDocumentToYaml } from './resource-document'
import { HTTP_PLUGIN_CATALOG } from '@/components/ResourceEditor/EdgionPlugins/pluginCatalog'
import { STREAM_PLUGIN_TYPES, TLS_ROUTE_PLUGIN_TYPES } from '@/types/edgion-stream-plugins'
import { isValidGep2257Duration, isValidHTTPHeaderName } from './validation'

export const DEFAULT_YAML = `apiVersion: edgion.io/v1alpha1
kind: EdgionGatewayConfig
metadata:
  name: default-config
spec:
  httpTimeout:
    client:
      readTimeout: "60s"
      writeTimeout: "60s"
    backend:
      defaultConnectTimeout: "5s"
      defaultRequestTimeout: "60s"
  retry:
    attempts: 2
  requestBody:
    defaultMaxBodySize: 32MiB
  preflightPolicy:
    mode: cors-standard
    statusCode: 204
`

export function createEmpty(): EdgionGatewayConfig {
  return {
    apiVersion: 'edgion.io/v1alpha1',
    kind: 'EdgionGatewayConfig',
    metadata: { name: 'default-config' },
    spec: {
      httpTimeout: {
        client: { readTimeout: '60s', writeTimeout: '60s' },
        backend: { defaultConnectTimeout: '5s', defaultRequestTimeout: '60s' },
      },
      retry: { attempts: 2 },
      requestBody: { defaultMaxBodySize: '32MiB' },
      preflightPolicy: { mode: 'cors-standard', statusCode: 204 },
    },
  }
}

export function normalize(raw: unknown): EdgionGatewayConfig {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('EdgionGatewayConfig document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'EdgionGatewayConfig') throw new Error('Expected an EdgionGatewayConfig document')
  return structuredClone(document) as unknown as EdgionGatewayConfig
}

export function toYaml(cfg: EdgionGatewayConfig): string {
  return dumpYaml(cfg)
}

export function toMutationYaml(cfg: EdgionGatewayConfig, mode: 'create' | 'update'): string {
  return mutationDocumentToYaml(cfg, 'edgiongatewayconfig', mode)
}

export function fromYaml(yamlStr: string): EdgionGatewayConfig {
  return normalize(yaml.load(yamlStr) as any)
}

const IP_GROUP_NAME_PATTERN = /^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,61}[A-Za-z0-9])?$/
const BYTE_SIZE_UNITS: Array<[string, number]> = [
  ['kib', 1024],
  ['kb', 1024],
  ['ki', 1024],
  ['k', 1024],
  ['mib', 1024 ** 2],
  ['mb', 1024 ** 2],
  ['mi', 1024 ** 2],
  ['m', 1024 ** 2],
  ['gib', 1024 ** 3],
  ['gb', 1024 ** 3],
  ['gi', 1024 ** 3],
  ['g', 1024 ** 3],
  ['b', 1],
]
const U64_MAX = Number((1n << 64n) - 1n)
const RUST_FLOAT_PATTERN = /^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?$/

export function parseEdgionByteSize(value: string): number | null {
  const normalized = value.trim().toLowerCase()
  if (!normalized) return null

  const unit = BYTE_SIZE_UNITS.find(([suffix]) => normalized.endsWith(suffix))
  const numericPart = (unit ? normalized.slice(0, -unit[0].length) : normalized).trim()
  if (!RUST_FLOAT_PATTERN.test(numericPart)) return null

  const numericValue = Number(numericPart)
  if (!Number.isFinite(numericValue) || numericValue < 0) return null
  return Math.min(Math.trunc(numericValue * (unit?.[1] ?? 1)), U64_MAX)
}

function validIpOrCidr(value: string): boolean {
  const [address, prefix] = value.split('/')
  if (!address) return false
  if (address.includes(':')) return prefix === undefined || (/^\d+$/.test(prefix) && Number(prefix) <= 128)
  const octets = address.split('.')
  return octets.length === 4 && octets.every((octet) => /^\d+$/.test(octet) && Number(octet) <= 255) && (prefix === undefined || (/^\d+$/.test(prefix) && Number(prefix) <= 32))
}

export function validateEdgionGatewayConfig(resource: EdgionGatewayConfig): string[] {
  const errors: string[] = []
  const spec = resource.spec || {}
  if (spec.maxRetries !== undefined) errors.push('spec.maxRetries was removed; use spec.retry.attempts')
  const attempts = spec.retry?.attempts
  if (attempts !== undefined && (!Number.isInteger(attempts) || attempts < 0 || attempts > 2147483647)) {
    errors.push('spec.retry.attempts must be an integer from 0 to 2147483647')
  }
  const remoteIpHeader = spec.forwardedHeaders?.remoteIpHeader
  if (remoteIpHeader !== undefined) {
    const reserved = ['connection', 'keep-alive', 'proxy-authenticate', 'proxy-authorization', 'te', 'trailer', 'transfer-encoding', 'upgrade', 'host', 'content-length', 'x-forwarded-for']
    if (typeof remoteIpHeader !== 'string' || remoteIpHeader.length > 256 || !isValidHTTPHeaderName(remoteIpHeader)) {
      errors.push('spec.forwardedHeaders.remoteIpHeader must be a valid HTTP header name')
    } else if (reserved.includes(remoteIpHeader.toLowerCase())) {
      errors.push('spec.forwardedHeaders.remoteIpHeader is reserved')
    }
  }
  const policy = spec.pluginPolicy
  if (policy != null && (typeof policy !== 'object' || Array.isArray(policy))) errors.push('spec.pluginPolicy must be an object')
  if (policy && typeof policy === 'object' && !Array.isArray(policy)) {
    const names = new Set([
      ...HTTP_PLUGIN_CATALOG.map((plugin) => `http/${plugin.type}`),
      ...STREAM_PLUGIN_TYPES.map((type) => `stream/${type}`),
      ...TLS_ROUTE_PLUGIN_TYPES.map((type) => `tls-route/${type}`),
    ])
    const validStatus = (value: unknown) => typeof value === 'number' && Number.isInteger(value) && value >= 400 && value <= 599
    if (policy.defaultAction !== undefined && !['allow', 'deny'].includes(policy.defaultAction)) errors.push('pluginPolicy.defaultAction must be allow or deny')
    if (policy.deniedAction !== undefined && !['block', 'bypass'].includes(policy.deniedAction)) errors.push('pluginPolicy.deniedAction must be block or bypass')
    if (policy.blockStatus !== undefined && !validStatus(policy.blockStatus)) errors.push('pluginPolicy.blockStatus must be an integer from 400 to 599')
    if (policy.allow != null) {
      if (!Array.isArray(policy.allow) || policy.allow.some((name) => !names.has(name))) errors.push('pluginPolicy.allow must contain canonical qualified plugin names')
      else if (new Set(policy.allow).size !== policy.allow.length) errors.push('pluginPolicy.allow contains duplicate names')
    }
    if (policy.deny !== undefined) {
      if (!Array.isArray(policy.deny)) errors.push('pluginPolicy.deny must be an array')
      else {
        const seen = new Set<string>()
        for (const entry of policy.deny) {
          if (!entry || typeof entry !== 'object' || !names.has(entry.name)) { errors.push('pluginPolicy.deny contains an invalid plugin name'); continue }
          if (seen.has(entry.name)) errors.push('pluginPolicy.deny contains duplicate names')
          seen.add(entry.name)
          if (entry.action != null && !['block', 'bypass'].includes(entry.action)) errors.push('pluginPolicy.deny action must be block or bypass')
          if (entry.blockStatus != null) {
            if (!validStatus(entry.blockStatus)) errors.push('pluginPolicy.deny blockStatus must be an integer from 400 to 599')
            if ((entry.action ?? policy.deniedAction ?? 'block') === 'bypass') errors.push('pluginPolicy.deny blockStatus must not be set for bypass')
          }
        }
      }
    }
  }
  for (const field of ['threads', 'workStealing', 'gracePeriodSeconds', 'gracefulShutdownTimeoutS', 'upstreamKeepalivePoolSize', 'errorLog']) {
    if (spec.server?.[field] !== undefined) errors.push(`spec.server.${field} is no longer a GatewayConfig field`)
  }
  if (spec.securityProtect?.rejectDuplicateHost !== undefined) errors.push('spec.securityProtect.rejectDuplicateHost was removed')
  const maxInstances = spec.linkSys?.maxInstancesPerKind
  if (maxInstances !== undefined && (!Number.isInteger(maxInstances) || maxInstances < 1 || maxInstances > 10000)) errors.push('spec.linkSys.maxInstancesPerKind must be an integer from 1 to 10000')
  const keepaliveLimit = spec.server?.downstreamKeepaliveRequestLimit
  if (keepaliveLimit !== undefined && (!Number.isInteger(keepaliveLimit) || keepaliveLimit < 0 || keepaliveLimit > 4294967295)) errors.push('spec.server.downstreamKeepaliveRequestLimit must be an integer from 0 to 4294967295')
  const durations: Array<[string, unknown]> = [
    ['spec.httpTimeout.client.readTimeout', spec.httpTimeout?.client?.readTimeout],
    ['spec.httpTimeout.client.writeTimeout', spec.httpTimeout?.client?.writeTimeout],
    ['spec.httpTimeout.client.keepaliveTimeout', spec.httpTimeout?.client?.keepaliveTimeout],
    ['spec.httpTimeout.backend.defaultConnectTimeout', spec.httpTimeout?.backend?.defaultConnectTimeout],
    ['spec.httpTimeout.backend.defaultRequestTimeout', spec.httpTimeout?.backend?.defaultRequestTimeout],
    ['spec.httpTimeout.backend.defaultIdleTimeout', spec.httpTimeout?.backend?.defaultIdleTimeout],
    ['spec.tcpTimeout.idleTimeout', spec.tcpTimeout?.idleTimeout],
    ['spec.tcpTimeout.connectTimeout', spec.tcpTimeout?.connectTimeout],
    ['spec.dnsResolver.cacheTtl', spec.dnsResolver?.cacheTtl],
    ['spec.requestBody.storageOperationTimeout', spec.requestBody?.storageOperationTimeout],
  ]
  durations.forEach(([path, value]) => {
    if (value !== undefined && (typeof value !== 'string' || !isValidGep2257Duration(value))) {
      errors.push(`${path} is not a valid GEP-2257 duration`)
    }
  })
  if (spec.maxBodySize !== undefined) errors.push('spec.maxBodySize was removed; use spec.requestBody.maxBodySize')
  const body = spec.requestBody ?? {}
  const sizes = ['defaultMemoryBufferSize', 'maxMemoryBufferSize', 'defaultMaxBodySize', 'maxBodySize'] as const
  for (const field of sizes) {
    const value = body[field]
    if (value !== undefined && (typeof value !== 'string' || (parseEdgionByteSize(value) ?? 0) <= 0)) {
      errors.push(`spec.requestBody.${field} is invalid (expected a positive byte size such as '32MiB')`)
    }
  }
  const memoryValue = body.maxMemoryBufferSize ?? body.defaultMemoryBufferSize ?? '128KiB'
  const bodyValue = body.maxBodySize ?? body.defaultMaxBodySize ?? '32MiB'
  const memorySize = typeof memoryValue === 'string' ? parseEdgionByteSize(memoryValue) : null
  const bodySize = typeof bodyValue === 'string' ? parseEdgionByteSize(bodyValue) : null
  if (memorySize !== null && bodySize !== null && memorySize > bodySize) {
    errors.push('Effective requestBody memory buffer size must not exceed the effective max body size')
  }
  if (body.enabled !== undefined && typeof body.enabled !== 'boolean') errors.push('spec.requestBody.enabled must be a boolean')
  if (typeof body.storageOperationTimeout === 'string' && isValidGep2257Duration(body.storageOperationTimeout)
    && !body.storageOperationTimeout.match(/\d+/g)?.some((part) => Number(part) > 0)) {
    errors.push('spec.requestBody.storageOperationTimeout must be greater than zero')
  }
  const degradeThreshold = spec.loadBalancing?.degradeThreshold
  if (degradeThreshold !== undefined && (!Number.isInteger(degradeThreshold) || degradeThreshold < 0 || degradeThreshold > 100)) {
    errors.push('spec.loadBalancing.degradeThreshold must be an integer from 0 to 100')
  }
  const realIp = spec.realIp
  if (realIp != null) {
    if (!Array.isArray(realIp.trustedIps) || realIp.trustedIps.length === 0) errors.push('spec.realIp.trustedIps requires at least one group')
    const header = realIp.realIpHeader
    if (header !== undefined && (typeof header !== 'string' || header.length > 256 || !isValidHTTPHeaderName(header))) {
      errors.push('spec.realIp.realIpHeader must be a valid HTTP header name')
    }
    const hops = realIp.maxTrustedHops
    if (hops != null && (!Number.isInteger(hops) || hops < 0 || hops > 4294967295)) errors.push('spec.realIp.maxTrustedHops must be an integer from 0 to 4294967295')
  }
  const groupNames = new Set<string>()
  const trustedGroups = Array.isArray(realIp?.trustedIps) ? realIp.trustedIps : []
  trustedGroups.forEach((group, index) => {
    const path = `spec.realIp.trustedIps[${index}]`
    if (!IP_GROUP_NAME_PATTERN.test(group.name || '')) errors.push(`${path}.name is invalid`)
    else if (groupNames.has(group.name)) errors.push(`${path}.name must be unique`)
    else groupNames.add(group.name)
    if (!group.cidrs?.length) errors.push(`${path}.cidrs requires at least one entry`)
    group.cidrs?.forEach((cidr, cidrIndex) => { if (!validIpOrCidr(cidr)) errors.push(`${path}.cidrs[${cidrIndex}] is invalid`) })
  })
  spec.globalPluginsRef?.forEach((ref, index) => { if (!ref.name?.trim()) errors.push(`spec.globalPluginsRef[${index}].name is required`) })
  const outbound = spec.outboundTls
  outbound?.validation?.caCertificateRefs?.forEach((ref, index) => {
    if (!ref.name?.trim()) errors.push(`spec.outboundTls.validation.caCertificateRefs[${index}].name is required`)
    if (!ref.namespace?.trim()) errors.push(`spec.outboundTls.validation.caCertificateRefs[${index}].namespace is required for a cluster-scoped resource`)
    if (!['Secret', 'ConfigMap'].includes(ref.kind || '')) errors.push(`spec.outboundTls.validation.caCertificateRefs[${index}].kind must be Secret or ConfigMap`)
  })
  if (outbound?.clientCertificateRef) {
    if (!outbound.clientCertificateRef.name?.trim()) errors.push('spec.outboundTls.clientCertificateRef.name is required')
    if (!outbound.clientCertificateRef.namespace?.trim()) errors.push('spec.outboundTls.clientCertificateRef.namespace is required for a cluster-scoped resource')
    if (outbound.clientCertificateRef.kind && outbound.clientCertificateRef.kind !== 'Secret') errors.push('spec.outboundTls.clientCertificateRef.kind must be Secret')
  }
  outbound?.validation?.subjectAltNames?.forEach((san, index) => {
    if (san.type === 'Hostname' && !san.hostname?.trim()) errors.push(`spec.outboundTls.validation.subjectAltNames[${index}].hostname is required`)
    if (san.type === 'URI' && !san.uri?.trim()) errors.push(`spec.outboundTls.validation.subjectAltNames[${index}].uri is required`)
  })
  const dns = spec.dnsResolver
  if (dns?.linkSysRef && dns.servers?.length) errors.push('spec.dnsResolver.linkSysRef and servers are mutually exclusive')
  if (dns?.linkSysRef && (!dns.linkSysRef.name?.trim() || !dns.linkSysRef.namespace?.trim())) errors.push('spec.dnsResolver.linkSysRef namespace and name are required')
  dns?.servers?.forEach((server, index) => { if (!server.trim()) errors.push(`spec.dnsResolver.servers[${index}] is required`) })
  const preflightStatus = spec.preflightPolicy?.statusCode
  if (preflightStatus !== undefined && (!Number.isInteger(preflightStatus) || preflightStatus < 200 || preflightStatus > 599)) errors.push('spec.preflightPolicy.statusCode must be an integer from 200 to 599')
  return errors
}
