/**
 * EdgionGatewayConfig 类型定义
 * apiVersion: edgion.io/v1alpha1
 * 集群级资源
 */

import type { K8sObjectMeta } from '@/types/gateway-api/common'

export interface EdgionGatewayConfigSpec {
  server?: {
    threads?: number
    workStealing?: boolean
    gracePeriodSeconds?: number
    gracefulShutdownTimeoutS?: number
    upstreamKeepalivePoolSize?: number
    enableCompression?: boolean
    downstreamKeepaliveRequestLimit?: number
  }
  httpTimeout?: {
    client?: { readTimeout?: string; writeTimeout?: string; keepaliveTimeout?: string }
    backend?: { defaultConnectTimeout?: string; defaultRequestTimeout?: string; defaultIdleTimeout?: string }
  }
  maxRetries?: number
  realIp?: {
    trustedIps?: string[]
    realIpHeader?: string
    recursive?: boolean
  }
  securityProtect?: {
    xForwardedForLimit?: number
    requireSniHostMatch?: boolean
    fallbackSni?: string
    tlsProxyLogRecord?: boolean
  }
  globalPluginsRef?: Array<{ name: string; namespace?: string }>
  preflightPolicy?: { mode?: string; statusCode?: number }
  enableReferenceGrantValidation?: boolean
}

export interface EdgionGatewayConfig {
  apiVersion: string
  kind: 'EdgionGatewayConfig'
  metadata: K8sObjectMeta
  spec: EdgionGatewayConfigSpec
  status?: any
}
