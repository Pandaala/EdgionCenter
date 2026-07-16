import * as yaml from 'js-yaml'
import type {
  ActiveHealthCheckConfig,
  EdgionBackendTrafficPolicy,
  LoadBalancerConfig,
  OutlierDetectionConfig,
  UpstreamAuthorityConfig,
} from '@/types/edgion-backend-traffic-policy'
import { dumpYaml } from './yaml-utils'
import { mutationDocumentToYaml } from './resource-document'

export function createDefaultLoadBalancer(): LoadBalancerConfig {
  return { type: 'RoundRobin' }
}

export function createDefaultActiveHealthCheck(): ActiveHealthCheckConfig {
  return {
    type: 'http',
    path: '/',
    interval: '10s',
    timeout: '3s',
    healthyThreshold: 2,
    unhealthyThreshold: 3,
    expectedStatuses: [200],
  }
}

export function createDefaultOutlierDetection(): OutlierDetectionConfig {
  return {
    consecutiveErrors: 5,
    ejectionSeconds: 30,
    maxEjectionPercent: 50,
  }
}

export function createDefaultUpstreamAuthority(): UpstreamAuthorityConfig {
  return { pattern: '*.example.internal', template: '${ctx:tenant}.example.internal' }
}

export function createEmptyEdgionBackendTrafficPolicy(): EdgionBackendTrafficPolicy {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'EdgionBackendTrafficPolicy',
    metadata: { name: '', namespace: 'default' },
    spec: { targetRefs: [{ group: '', kind: 'Service', name: '' }] },
  }
}

export function normalizeEdgionBackendTrafficPolicy(raw: unknown): EdgionBackendTrafficPolicy {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new Error('EdgionBackendTrafficPolicy document must be an object')
  }
  const document = raw as Record<string, unknown>
  if (document.kind !== 'EdgionBackendTrafficPolicy') {
    throw new Error('Expected an EdgionBackendTrafficPolicy document')
  }
  if (!document.metadata || typeof document.metadata !== 'object') {
    throw new Error('EdgionBackendTrafficPolicy document must contain metadata')
  }
  if (!document.spec || typeof document.spec !== 'object') {
    throw new Error('EdgionBackendTrafficPolicy document must contain spec')
  }
  return structuredClone(document) as unknown as EdgionBackendTrafficPolicy
}

export function edgionBackendTrafficPolicyToYaml(policy: EdgionBackendTrafficPolicy): string {
  return dumpYaml(policy)
}

export function edgionBackendTrafficPolicyFromYaml(source: string): EdgionBackendTrafficPolicy {
  return normalizeEdgionBackendTrafficPolicy(yaml.load(source))
}

function isPositiveInteger(value: number): boolean {
  return Number.isInteger(value) && value >= 1
}

function isNonZeroDuration(value: string): boolean {
  const compact = value.trim().replace(/\s+/g, '')
  if (!compact || compact.startsWith('-')) return false
  const token = /(\d+(?:\.\d+)?)(milliseconds?|millis|ms|seconds?|secs?|sec|s|minutes?|mins?|min|m|hours?|hrs?|hr|h|days?|d)?/gy
  let offset = 0
  let totalMilliseconds = 0
  let components = 0
  while (offset < compact.length) {
    token.lastIndex = offset
    const match = token.exec(compact)
    if (!match || match.index !== offset) return false
    const number = Number(match[1])
    const unit = match[2]
    if (!unit && compact.length !== match[0].length) return false
    const multiplier = unit?.startsWith('ms') || unit?.startsWith('milli') ? 1
      : unit?.startsWith('m') ? 60_000
      : unit?.startsWith('h') ? 3_600_000
      : unit?.startsWith('d') ? 86_400_000
      : 1_000
    totalMilliseconds += number * multiplier
    offset = token.lastIndex
    components += 1
  }
  return components > 0 && totalMilliseconds > 0
}

function validateDuration(value: string | undefined, field: string, errors: string[]) {
  if (value !== undefined && !isNonZeroDuration(value)) {
    errors.push(`${field} must be a valid non-zero duration`)
  }
}

function patternSuffix(pattern: string): string | null {
  if (!pattern.startsWith('*.') || pattern.slice(2).includes('*')) return null
  const suffix = pattern.slice(2)
  const labels = suffix.split('.')
  if (!suffix || labels.some((label) => !/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/i.test(label))) return null
  if (/^\d+$/.test(labels[labels.length - 1])) return null
  return `.${suffix}`
}

export function validateEdgionBackendTrafficPolicy(policy: EdgionBackendTrafficPolicy): string[] {
  const errors: string[] = []
  const refs = policy.spec.targetRefs
  if (!Array.isArray(refs) || refs.length === 0) errors.push('targetRefs must not be empty')
  refs?.forEach((ref, index) => {
    if (!ref.name) errors.push(`targetRefs[${index}].name must not be empty`)
    if (!(ref.group === undefined || ref.group === '' || ref.group === 'core') || ref.kind !== 'Service') {
      errors.push(`targetRefs[${index}] must reference a core Service`)
    }
  })

  const lb = policy.spec.loadBalancer
  if (lb) {
    if (!['RoundRobin', 'LeastConn', 'Ewma', 'ConsistentHash'].includes(lb.type)) {
      errors.push('loadBalancer.type is invalid')
    }
    if (lb.type === 'ConsistentHash') {
      if (!lb.consistentHash) errors.push('loadBalancer.consistentHash is required')
      else {
        if (!['header', 'cookie', 'queryParam'].includes(lb.consistentHash.hashOn)) errors.push('consistentHash.hashOn is invalid')
        if (!lb.consistentHash.key) errors.push('consistentHash.key must not be empty')
      }
    } else if (lb.consistentHash) errors.push('consistentHash is only valid for ConsistentHash')
    if (lb.panicThreshold !== undefined && (!Number.isInteger(lb.panicThreshold) || lb.panicThreshold < 0 || lb.panicThreshold > 100)) {
      errors.push('loadBalancer.panicThreshold must be 0-100')
    }
  }

  const active = policy.spec.healthCheck?.active
  if (active) {
    if (active.type !== undefined && !['http', 'tcp', 'grpc'].includes(active.type)) errors.push('healthCheck.active.type is invalid')
    if (active.healthyThreshold !== undefined && !isPositiveInteger(active.healthyThreshold)) errors.push('healthyThreshold must be >= 1')
    if (active.unhealthyThreshold !== undefined && !isPositiveInteger(active.unhealthyThreshold)) errors.push('unhealthyThreshold must be >= 1')
    validateDuration(active.interval, 'interval', errors)
    validateDuration(active.timeout, 'timeout', errors)
    if (active.port !== undefined && (!Number.isInteger(active.port) || active.port < 0 || active.port > 65535)) errors.push('port must be 0-65535')
    if ((active.type ?? 'http') === 'http') {
      if (active.path === '') errors.push('path must not be empty for http health check')
      if (active.expectedStatuses?.length === 0) errors.push('expectedStatuses must not be empty for http health check')
      if (active.expectedStatuses?.some((status) => !Number.isInteger(status) || status < 100 || status > 599)) {
        errors.push('expectedStatuses must contain valid HTTP status codes')
      }
    }
  }

  const outlier = policy.spec.outlierDetection
  if (outlier) {
    if (outlier.consecutiveErrors !== undefined && !isPositiveInteger(outlier.consecutiveErrors)) errors.push('consecutiveErrors must be >= 1')
    if (outlier.consecutiveGatewayErrors !== undefined && !isPositiveInteger(outlier.consecutiveGatewayErrors)) errors.push('consecutiveGatewayErrors must be >= 1')
    if (outlier.consecutiveLocalOriginFailures !== undefined && !isPositiveInteger(outlier.consecutiveLocalOriginFailures)) errors.push('consecutiveLocalOriginFailures must be >= 1')
    if (outlier.ejectionSeconds !== undefined && !isPositiveInteger(outlier.ejectionSeconds)) errors.push('ejectionSeconds must be >= 1')
    if (outlier.maxEjectionPercent !== undefined && (!Number.isInteger(outlier.maxEjectionPercent) || outlier.maxEjectionPercent < 0 || outlier.maxEjectionPercent > 100)) errors.push('maxEjectionPercent must be 0-100')
  }

  const authority = policy.spec.upstreamAuthority
  if (authority) {
    const suffix = patternSuffix(authority.pattern)
    if (!suffix) errors.push('upstreamAuthority.pattern must be one wildcard DNS label with a valid suffix')
    else {
      if (!authority.template.toLowerCase().endsWith(suffix.toLowerCase())) errors.push('upstreamAuthority.template must end with the pattern suffix')
      else {
        const slot = authority.template.slice(0, authority.template.length - suffix.length)
        const variables = [...slot.matchAll(/\$\{[^{}]+\}/g)]
        const literal = slot.replace(/\$\{[^{}]+\}/g, '')
        const firstVariableIndex = variables[0]?.index
        const lastVariable = variables[variables.length - 1]
        const lastVariableEnd = lastVariable && lastVariable.index !== undefined
          ? lastVariable.index + lastVariable[0].length
          : -1
        const startsWithInvalidLiteral = firstVariableIndex !== 0 && slot.startsWith('-')
        const endsWithInvalidLiteral = lastVariableEnd !== slot.length && slot.endsWith('-')
        if (!slot || variables.length === 0 || /[.${}]/.test(literal) || !/^[a-z0-9-]*$/i.test(literal)
          || startsWithInvalidLiteral || endsWithInvalidLiteral) {
          errors.push('upstreamAuthority.template label slot must contain a variable and only valid DNS-label literals')
        }
      }
      if (authority.healthCheckHost) {
        const label = authority.healthCheckHost.toLowerCase().endsWith(suffix.toLowerCase())
          ? authority.healthCheckHost.slice(0, authority.healthCheckHost.length - suffix.length)
          : ''
        if (!/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/i.test(label)) errors.push('upstreamAuthority.healthCheckHost must match pattern')
      }
    }
    if (active && !authority.healthCheckHost) errors.push('upstreamAuthority.healthCheckHost is required with an active health check')
  }
  return errors
}

export function edgionBackendTrafficPolicyToMutationYaml(
  policy: EdgionBackendTrafficPolicy,
  mode: 'create' | 'update',
): string {
  const errors = validateEdgionBackendTrafficPolicy(policy)
  if (errors.length > 0) throw new Error(errors.join('; '))
  return mutationDocumentToYaml(policy, 'edgionbackendtrafficpolicy', mode)
}
