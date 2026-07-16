import * as yaml from 'js-yaml'
import type { EdgionGatewayConfig } from '@/types/edgion-gateway-config'
import { dumpYaml } from './yaml-utils'
import { mutationDocumentToYaml } from './resource-document'

export const DEFAULT_YAML = `apiVersion: edgion.io/v1alpha1
kind: EdgionGatewayConfig
metadata:
  name: default-config
spec:
  server:
    gracePeriodSeconds: 30
    gracefulShutdownTimeoutS: 10
  httpTimeout:
    client:
      readTimeout: "60s"
      writeTimeout: "60s"
    backend:
      defaultConnectTimeout: "5s"
      defaultRequestTimeout: "60s"
  maxRetries: 3
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
      server: { gracePeriodSeconds: 30 },
      httpTimeout: {
        client: { readTimeout: '60s', writeTimeout: '60s' },
        backend: { defaultConnectTimeout: '5s', defaultRequestTimeout: '60s' },
      },
      maxRetries: 3,
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

const DURATION_PATTERN = /^([0-9]+(\.[0-9]+)?(milliseconds|millisecond|millis|ms|seconds|second|secs|sec|s|minutes|minute|mins|min|m|hours|hour|hrs|hr|h|days|day|d))+$/
const IP_GROUP_NAME_PATTERN = /^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,61}[A-Za-z0-9])?$/

function validDuration(value: unknown): boolean {
  return typeof value === 'string' && (DURATION_PATTERN.test(value) || /^[0-9]+(?:\.[0-9]+)?$/.test(value))
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
  ]
  durations.forEach(([path, value]) => { if (value !== undefined && !validDuration(value)) errors.push(`${path} is not a valid duration`) })
  if (spec.loadBalancing?.panicThreshold !== undefined && (spec.loadBalancing.panicThreshold < 0 || spec.loadBalancing.panicThreshold > 100)) errors.push('spec.loadBalancing.panicThreshold must be 0-100')
  const groupNames = new Set<string>()
  spec.realIp?.trustedIps?.forEach((group, index) => {
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
  if (spec.preflightPolicy?.statusCode !== undefined && (spec.preflightPolicy.statusCode < 100 || spec.preflightPolicy.statusCode > 599)) errors.push('spec.preflightPolicy.statusCode must be 100-599')
  return errors
}
