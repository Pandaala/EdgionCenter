/**
 * EdgionGatewayConfig 类型定义
 * apiVersion: edgion.io/v1alpha1
 * 集群级资源
 */

import type { K8sObjectMeta } from '@/types/gateway-api/common'

export interface IpGroup {
  name: string
  description?: string
  cidrs: string[]
  [key: string]: unknown
}

export interface ObjectReference {
  group?: string
  kind?: string
  namespace?: string
  name: string
  [key: string]: unknown
}

export interface SubjectAltName {
  type: 'Hostname' | 'URI'
  hostname?: string
  uri?: string
  [key: string]: unknown
}

export interface GatewayConfigUnmaskedKeys {
  header?: string[]
  respHeader?: string[]
  query?: string[]
  cookie?: string[]
  ctx?: string[]
  [key: string]: unknown
}

export interface GatewayConfigAccessLogExtern {
  unmaskedKeys?: GatewayConfigUnmaskedKeys
  [key: string]: unknown
}

export interface EdgionGatewayConfigSpec {
  server?: {
    threads?: number
    workStealing?: boolean
    gracePeriodSeconds?: number
    gracefulShutdownTimeoutS?: number
    upstreamKeepalivePoolSize?: number
    errorLog?: string
    enableCompression?: boolean
    downstreamKeepaliveRequestLimit?: number
    [key: string]: unknown
  }
  httpTimeout?: {
    client?: { readTimeout?: string; writeTimeout?: string; keepaliveTimeout?: string }
    backend?: { defaultConnectTimeout?: string; defaultRequestTimeout?: string; defaultIdleTimeout?: string }
  }
  maxRetries?: number
  maxBodySize?: string
  tcpTimeout?: { idleTimeout?: string; connectTimeout?: string; [key: string]: unknown }
  loadBalancing?: { panicThreshold?: number; [key: string]: unknown }
  realIp?: {
    trustedIps?: IpGroup[]
    realIpHeader?: string
    recursive?: boolean
    maxTrustedHops?: number
    [key: string]: unknown
  }
  securityProtect?: {
    xForwardedForLimit?: number
    requireSniHostMatch?: boolean
    fallbackSni?: string
    tlsProxyLogRecord?: boolean
    allowLoopbackUpstream?: boolean
    rejectDuplicateHost?: boolean
    [key: string]: unknown
  }
  globalPluginsRef?: Array<{ name: string; namespace?: string; [key: string]: unknown }>
  accessLogExtern?: GatewayConfigAccessLogExtern
  preflightPolicy?: { mode?: 'cors-standard' | 'all-options'; statusCode?: number; [key: string]: unknown }
  linkSys?: { webhookMaxResponseBytes?: number; [key: string]: unknown }
  outboundTls?: {
    verify?: boolean
    validation?: {
      caCertificateRefs?: ObjectReference[]
      wellKnownCACertificates?: 'System'
      hostname?: string
      subjectAltNames?: SubjectAltName[]
      [key: string]: unknown
    }
    clientCertificateRef?: ObjectReference
    [key: string]: unknown
  }
  dnsResolver?: {
    linkSysRef?: { namespace: string; name: string; [key: string]: unknown }
    servers?: string[]
    cacheTtl?: string
    [key: string]: unknown
  }
  [key: string]: unknown
}

export interface EdgionGatewayConfig {
  apiVersion: string
  kind: 'EdgionGatewayConfig'
  metadata: K8sObjectMeta
  spec: EdgionGatewayConfigSpec
  status?: any
  [key: string]: unknown
}
