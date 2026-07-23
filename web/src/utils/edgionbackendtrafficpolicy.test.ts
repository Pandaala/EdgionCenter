import * as yaml from 'js-yaml'
import { describe, expect, it } from 'vitest'
import {
  createDefaultCircuitBreaker,
  createDefaultConnectionOverride,
  createDefaultOutlierDetection,
  createDefaultRetryConstraint,
  edgionBackendTrafficPolicyFromYaml,
  edgionBackendTrafficPolicyToMutationYaml,
  edgionBackendTrafficPolicyToYaml,
  normalizeEdgionBackendTrafficPolicy,
  validateEdgionBackendTrafficPolicy,
} from './edgionbackendtrafficpolicy'
import type { EdgionBackendTrafficPolicy } from '@/types/edgion-backend-traffic-policy'

const fullPolicy: EdgionBackendTrafficPolicy = {
  apiVersion: 'edgion.io/v1',
  kind: 'EdgionBackendTrafficPolicy',
  metadata: {
    name: 'payments',
    namespace: 'prod',
    labels: { owner: 'platform' },
    resourceVersion: '12',
    creationTimestamp: '2026-07-15T00:00:00Z',
  },
  spec: {
    targetRefs: [
      { group: '', kind: 'Service', name: 'payments', futureRef: 'keep' },
      { group: 'core', kind: 'Service', name: 'payments-canary' },
    ],
    loadBalancer: {
      type: 'ConsistentHash',
      consistentHash: { hashOn: 'queryParam', key: 'tenant', futureHash: true },
      panicThreshold: 30,
      futureLoadBalancer: 'keep',
    },
    healthCheck: {
      active: {
        type: 'grpc',
        path: '/retained-even-when-hidden',
        port: 9090,
        interval: '5s',
        timeout: '1s',
        healthyThreshold: 3,
        unhealthyThreshold: 4,
        expectedStatuses: [200, 204],
        host: 'probe-header.internal',
        grpcServiceName: 'acme.health.v1.Service',
        futureProbe: false,
      },
    },
    outlierDetection: {
      consecutiveErrors: 5,
      consecutiveGatewayErrors: 6,
      consecutiveLocalOriginFailures: 7,
      ejectionTime: '30s',
      maxEjectionTime: '10m',
      maxEjectionPercent: 40,
      futureOutlier: 0,
    },
    upstreamAuthority: {
      pattern: '*.sandbox.internal',
      template: '${ctx:tenant}.sandbox.internal',
      healthCheckHost: 'probe.sandbox.internal',
      futureAuthority: '',
    },
    retryConstraint: {
      budget: { percent: 20, interval: '10s', futureBudget: true },
      minRetryRate: { count: 10, interval: '1s', futureFloor: true },
      futureRetry: true,
    },
    circuitBreaker: { maxParallelRequests: 100, futureCircuit: true },
    connection: { connectTimeout: '5s', futureConnection: true },
    futureSpec: { enabled: false, entries: [] },
  },
  status: { conditions: [{ type: 'Accepted', status: 'True' }] },
}

describe('EdgionBackendTrafficPolicy adapter', () => {
  it('round-trips every current and unknown operator field without projection', () => {
    const normalized = normalizeEdgionBackendTrafficPolicy(fullPolicy)
    expect(edgionBackendTrafficPolicyFromYaml(edgionBackendTrafficPolicyToYaml(normalized))).toEqual(fullPolicy)
  })

  it('uses the common mutation boundary for form and YAML documents', () => {
    const fromForm = yaml.load(edgionBackendTrafficPolicyToMutationYaml(fullPolicy, 'update')) as any
    const fromYaml = yaml.load(edgionBackendTrafficPolicyToMutationYaml(
      edgionBackendTrafficPolicyFromYaml(edgionBackendTrafficPolicyToYaml(fullPolicy)),
      'update',
    )) as any

    expect(fromYaml).toEqual(fromForm)
    expect(fromForm.status).toBeUndefined()
    expect(fromForm.metadata.resourceVersion).toBe('12')
    expect(fromForm.metadata.creationTimestamp).toBeUndefined()
    expect(fromForm.spec.futureSpec).toEqual({ enabled: false, entries: [] })
    expect(fromForm.spec.targetRefs).toHaveLength(2)
    expect(fromForm.spec.healthCheck.active.futureProbe).toBe(false)
    expect(fromForm.spec.retryConstraint.futureRetry).toBe(true)
    expect(fromForm.spec.connection.futureConnection).toBe(true)
  })

  it('uses the Edgion defaults when optional resilience sections are enabled', () => {
    expect(createDefaultRetryConstraint()).toEqual({
      budget: { percent: 20, interval: '10s' },
      minRetryRate: { count: 10, interval: '1s' },
    })
    expect(createDefaultCircuitBreaker()).toEqual({ maxParallelRequests: 1 })
    expect(createDefaultConnectionOverride()).toEqual({ connectTimeout: '10s' })
    expect(createDefaultOutlierDetection()).toEqual({
      consecutiveErrors: 5,
      ejectionTime: '30s',
      maxEjectionPercent: 50,
    })
  })

  it.each(['RoundRobin', 'LeastConn', 'Ewma'] as const)('accepts the %s algorithm without consistentHash', (type) => {
    const policy = structuredClone(fullPolicy)
    policy.spec.loadBalancer = { type }
    expect(validateEdgionBackendTrafficPolicy(policy)).toEqual([])
  })

  it('validates conditional sections and complete health-check/outlier bounds', () => {
    const policy = structuredClone(fullPolicy)
    policy.spec.loadBalancer = { type: 'ConsistentHash', panicThreshold: 101 }
    policy.spec.healthCheck!.active = {
      type: 'http', path: '', interval: '0s', timeout: 'bad', healthyThreshold: 0,
      unhealthyThreshold: 0, expectedStatuses: [99, 600], port: 65536,
    }
    policy.spec.outlierDetection = {
      consecutiveErrors: 0,
      consecutiveGatewayErrors: 0,
      consecutiveLocalOriginFailures: 0,
      ejectionTime: '500ms',
      maxEjectionTime: 'bad',
      maxEjectionPercent: 101,
    }
    policy.spec.retryConstraint = {
      budget: { percent: 101, interval: '999ms' },
      minRetryRate: { count: 0, interval: '0s' },
    }
    policy.spec.circuitBreaker = { maxParallelRequests: 0 }
    policy.spec.connection = { connectTimeout: '1h1ms' }
    policy.spec.upstreamAuthority = {
      pattern: 'sandbox.internal',
      template: '${ctx:tenant}.wrong.internal',
    }
    const errors = validateEdgionBackendTrafficPolicy(policy).join('\n')
    expect(errors).toContain('consistentHash is required')
    expect(errors).toContain('panicThreshold must be 0-100')
    expect(errors).toContain('healthyThreshold must be >= 1')
    expect(errors).toContain('expectedStatuses must contain valid HTTP status codes')
    expect(errors).toContain('consecutiveLocalOriginFailures must be >= 1')
    expect(errors).toContain('ejectionTime must be a valid GEP-2257 duration of at least 1s')
    expect(errors).toContain('maxEjectionTime must be a valid GEP-2257 duration')
    expect(errors).toContain('maxEjectionPercent must be 0-100')
    expect(errors).toContain('retryConstraint.budget.interval must be in [1s, 1h]')
    expect(errors).toContain('retryConstraint.minRetryRate.count must be in 1-1000000')
    expect(errors).toContain('circuitBreaker.maxParallelRequests must be >= 1')
    expect(errors).toContain('connection.connectTimeout must be in (0s, 1h]')
    expect(errors).toContain('upstreamAuthority.pattern')
    expect(errors).toContain('healthCheckHost is required')
  })

  it('accepts Controller duration formats, sibling defaults, and safe mixed authority labels', () => {
    const policy = structuredClone(fullPolicy)
    policy.spec.healthCheck!.active = {
      interval: '1h30m',
      timeout: '500ms',
      path: '/',
      port: 1,
    }
    policy.spec.outlierDetection = { ejectionTime: '1s', maxEjectionTime: '500ms' }
    policy.spec.upstreamAuthority = {
      pattern: '*.sandbox.internal',
      template: 'tenant-${ctx:id}.sandbox.internal',
      healthCheckHost: 'probe.sandbox.internal',
    }
    expect(validateEdgionBackendTrafficPolicy(policy)).toEqual([])
  })

  it('supports sourceIp consistent hashing only when key is absent', () => {
    const policy = structuredClone(fullPolicy)
    policy.spec.loadBalancer = {
      type: 'ConsistentHash',
      consistentHash: { hashOn: 'sourceIp', futureHash: true },
    }
    expect(validateEdgionBackendTrafficPolicy(policy)).toEqual([])

    policy.spec.loadBalancer.consistentHash!.key = 'x-forwarded-for'
    expect(validateEdgionBackendTrafficPolicy(policy)).toContain(
      'consistentHash.key must be absent for sourceIp',
    )
  })

  it('accepts exact retry, connection, and health-port boundaries', () => {
    const policy = structuredClone(fullPolicy)
    policy.spec.retryConstraint = {
      budget: { percent: 0, interval: '1s' },
      minRetryRate: { count: 1, interval: '1ms' },
    }
    policy.spec.connection = { connectTimeout: '1ms' }
    policy.spec.healthCheck!.active!.port = 1
    expect(validateEdgionBackendTrafficPolicy(policy)).toEqual([])

    policy.spec.retryConstraint = {
      budget: { percent: 100, interval: '1h' },
      minRetryRate: { count: 1_000_000, interval: '1h' },
    }
    policy.spec.connection = { connectTimeout: '1h' }
    policy.spec.healthCheck!.active!.port = 65535
    expect(validateEdgionBackendTrafficPolicy(policy)).toEqual([])
  })

  it('rejects null minRetryRate instead of treating it as disabled', () => {
    const policy = structuredClone(fullPolicy)
    policy.spec.retryConstraint = {
      budget: { percent: 20, interval: '10s' },
      minRetryRate: null,
    } as any
    const errors = validateEdgionBackendTrafficPolicy(policy)
    expect(errors).toContain('retryConstraint.minRetryRate.count must be in 1-1000000')
    expect(errors).toContain('retryConstraint.minRetryRate.interval must be in (0s, 1h]')
  })
})
