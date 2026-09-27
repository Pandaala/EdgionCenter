/**
 * EdgionTls 类型定义
 * apiVersion: edgion.io/v1
 */

import type { K8sObjectMeta, Hostname } from '@/types/gateway-api/common'

export interface ObjectRef {
  name: string
  namespace?: string
  group?: string
  kind?: string
  [key: string]: unknown
}

export type ClientAuthMode = 'Terminate' | 'Mutual' | 'OptionalMutual'

export interface AllowedSan {
  type: 'DNS' | 'URI' | 'Email' | 'IP' | 'OtherName'
  match?: 'Exact' | 'Prefix' | 'Suffix' | 'Contains' | 'RegularExpression'
  value: string
  ignoreCase?: boolean
  oid?: string
}

export interface ClientAuth {
  mode?: ClientAuthMode
  caSecretRef?: ObjectRef
  verifyDepth?: number
  allowedSans?: AllowedSan[]
  allowedCns?: string[]
}

export interface EdgionTlsSpec {
  parentRefs?: Array<ObjectRef & { sectionName?: string; port?: number }>
  hosts: Hostname[]
  secretRef: ObjectRef
  clientAuth?: ClientAuth
  minTlsVersion?: string
  ciphers?: string[]
  [key: string]: unknown
}

export interface EdgionTls {
  apiVersion: string
  kind: 'EdgionTls'
  metadata: K8sObjectMeta
  spec: EdgionTlsSpec
  status?: any
  [key: string]: unknown
}
