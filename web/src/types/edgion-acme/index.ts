/**
 * EdgionAcme 类型定义
 * apiVersion: edgion.io/v1
 */

import type { K8sObjectMeta } from '@/types/gateway-api/common'

export type ChallengeType = 'http-01' | 'dns-01'

export interface Http01Config {
  gatewayRef?: { name: string; namespace?: string }
}

export interface Dns01Config {
  provider?: string
  credentialRef?: { name: string; namespace?: string }
  propagationTimeout?: number
  propagationCheckInterval?: number
}

export interface EdgionAcmeSpec {
  email: string
  domains: string[]
  server?: string
  keyType?: string
  challenge: {
    type: ChallengeType
    http01?: Http01Config
    dns01?: Dns01Config
  }
  storage: {
    secretName: string
    secretNamespace?: string
  }
  renewal?: {
    renewBeforeDays?: number
    checkInterval?: number
    failBackoff?: number
  }
  autoEdgionTls?: {
    enabled?: boolean
    name?: string
    parentRefs?: Array<{ name: string; namespace?: string }>
  }
}

export interface EdgionAcme {
  apiVersion: string
  kind: 'EdgionAcme'
  metadata: K8sObjectMeta
  spec: EdgionAcmeSpec
  status?: any
}
