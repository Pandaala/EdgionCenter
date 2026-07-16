import type { K8sObjectMeta } from '@/types/gateway-api/common'

export type AcmeKeyType = 'ecdsa-p256' | 'ecdsa-p384'
export type ChallengeType = 'http-01' | 'dns-01'

export interface ObjectReference {
  name: string
  namespace?: string
  group?: string
  kind?: string
  [key: string]: unknown
}

export interface ParentReference extends ObjectReference {
  sectionName?: string
  port?: number
}

export interface Http01Challenge {
  type: 'http-01'
  gatewayRef: ParentReference
  [key: string]: unknown
}

export interface Dns01Challenge {
  type: 'dns-01'
  provider: string
  credentialRef: ObjectReference
  propagationTimeout?: number
  propagationCheckInterval?: number
  [key: string]: unknown
}

export type AcmeChallenge = Http01Challenge | Dns01Challenge

export interface EdgionAcmeSpec {
  server?: string
  email: string
  privateKeySecretRef: ObjectReference
  domains: string[]
  keyType?: AcmeKeyType
  challenge: AcmeChallenge
  renewal?: {
    renewBeforeDays?: number
    checkInterval?: number
    failBackoff?: number
    [key: string]: unknown
  }
  externalAccountBinding?: {
    keyId: string
    keySecretRef: ObjectReference
    [key: string]: unknown
  }
  storage: {
    secretName: string
    secretNamespace?: string
    [key: string]: unknown
  }
  autoEdgionTls?: {
    enabled?: boolean
    name?: string
    parentRefs?: ParentReference[]
    [key: string]: unknown
  }
  [key: string]: unknown
}

export interface EdgionAcme {
  apiVersion: string
  kind: 'EdgionAcme'
  metadata: K8sObjectMeta & Record<string, unknown>
  spec: EdgionAcmeSpec
  status?: unknown
  [key: string]: unknown
}
