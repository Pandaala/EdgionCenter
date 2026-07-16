import * as yaml from 'js-yaml'
import { describe, expect, it } from 'vitest'
import {
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
      ejectionSeconds: 30,
      maxEjectionSeconds: 600,
      maxEjectionPercent: 40,
      futureOutlier: 0,
    },
    upstreamAuthority: {
      pattern: '*.sandbox.internal',
      template: '${ctx:tenant}.sandbox.internal',
      healthCheckHost: 'probe.sandbox.internal',
      futureAuthority: '',
    },
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
      ejectionSeconds: 0,
      maxEjectionSeconds: 0,
      maxEjectionPercent: 101,
    }
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
    expect(errors).toContain('maxEjectionPercent must be 0-100')
    expect(errors).toContain('upstreamAuthority.pattern')
    expect(errors).toContain('healthCheckHost is required')
  })

  it('accepts Controller duration formats, sibling defaults, and safe mixed authority labels', () => {
    const policy = structuredClone(fullPolicy)
    policy.spec.healthCheck!.active = {
      interval: '1h30m',
      timeout: '500millis',
      path: '/',
      port: 0,
    }
    policy.spec.outlierDetection = { maxEjectionSeconds: 0 }
    policy.spec.upstreamAuthority = {
      pattern: '*.sandbox.internal',
      template: 'tenant-${ctx:id}.sandbox.internal',
      healthCheckHost: 'probe.sandbox.internal',
    }
    expect(validateEdgionBackendTrafficPolicy(policy)).toEqual([])
  })
})
