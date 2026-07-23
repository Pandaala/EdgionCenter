import type { K8sMetadata } from '@/api/types'

export type LoadBalancerType = 'RoundRobin' | 'LeastConn' | 'Ewma' | 'ConsistentHash'
export type ConsistentHashOn = 'header' | 'cookie' | 'queryParam' | 'sourceIp'
export type HealthCheckType = 'http' | 'tcp' | 'grpc'

export interface PolicyTargetRef {
  group?: string
  kind: string
  name: string
  [key: string]: unknown
}

export interface ConsistentHashConfig {
  hashOn: ConsistentHashOn
  key?: string
  [key: string]: unknown
}

export interface LoadBalancerConfig {
  type: LoadBalancerType
  consistentHash?: ConsistentHashConfig
  panicThreshold?: number
  [key: string]: unknown
}

export interface ActiveHealthCheckConfig {
  type?: HealthCheckType
  path?: string
  port?: number
  interval?: string
  timeout?: string
  healthyThreshold?: number
  unhealthyThreshold?: number
  expectedStatuses?: number[]
  host?: string
  grpcServiceName?: string
  [key: string]: unknown
}

export interface ServiceHealthCheck {
  active?: ActiveHealthCheckConfig
  [key: string]: unknown
}

export interface OutlierDetectionConfig {
  consecutiveErrors?: number
  consecutiveGatewayErrors?: number
  consecutiveLocalOriginFailures?: number
  ejectionTime?: string
  maxEjectionTime?: string
  maxEjectionPercent?: number
  [key: string]: unknown
}

export interface RetryBudget {
  percent: number
  interval: string
  [key: string]: unknown
}

export interface RetryRateThreshold {
  count: number
  interval: string
  [key: string]: unknown
}

export interface RetryConstraintConfig {
  budget: RetryBudget
  minRetryRate: RetryRateThreshold
  [key: string]: unknown
}

export interface CircuitBreakerConfig {
  maxParallelRequests: number
  [key: string]: unknown
}

export interface ConnectionOverride {
  connectTimeout?: string
  [key: string]: unknown
}

export interface UpstreamAuthorityConfig {
  pattern: string
  template: string
  healthCheckHost?: string
  [key: string]: unknown
}

export interface EdgionBackendTrafficPolicySpec {
  targetRefs: PolicyTargetRef[]
  loadBalancer?: LoadBalancerConfig
  healthCheck?: ServiceHealthCheck
  outlierDetection?: OutlierDetectionConfig
  upstreamAuthority?: UpstreamAuthorityConfig
  retryConstraint?: RetryConstraintConfig
  circuitBreaker?: CircuitBreakerConfig
  connection?: ConnectionOverride
  [key: string]: unknown
}

export interface EdgionBackendTrafficPolicy {
  apiVersion: string
  kind: 'EdgionBackendTrafficPolicy'
  metadata: K8sMetadata
  spec: EdgionBackendTrafficPolicySpec
  status?: unknown
  [key: string]: unknown
}
