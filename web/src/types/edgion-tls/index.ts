/**
 * EdgionTls 类型定义
 * apiVersion: edgion.io/v1
 */

import type { K8sObjectMeta, Hostname } from '@/types/gateway-api/common'

export interface ObjectRef {
  name: string
  namespace?: string
}

export type ClientAuthMode = 'Terminate' | 'Mutual' | 'OptionalMutual'

export interface ClientAuth {
  mode?: ClientAuthMode
  caSecretRef?: ObjectRef
  verifyDepth?: number
  allowedSans?: string[]
  allowedCns?: string[]
}

export interface EdgionTlsSpec {
  parentRefs?: Array<{ name: string; namespace?: string }>
  hosts: Hostname[]
  secretRef: ObjectRef
  clientAuth?: ClientAuth
  minTlsVersion?: string
  cipherSuites?: string[]
}

export interface EdgionTls {
  apiVersion: string
  kind: 'EdgionTls'
  metadata: K8sObjectMeta
  spec: EdgionTlsSpec
  status?: any
}
