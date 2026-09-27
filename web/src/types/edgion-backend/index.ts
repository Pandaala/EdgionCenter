import type { K8sMetadata } from '@/api/types'

export type AiProvider = 'OpenAI' | 'OpenAICompatible' | 'Anthropic'

export interface AiCredential {
  name: string
  weight?: number
  secretRef: { name: string; namespace?: string; [key: string]: unknown }
  limits?: { requestsPerMinute?: number; tokensPerMinute?: number; [key: string]: unknown }
  [key: string]: unknown
}

export interface AiModel {
  name: string
  aliases?: string[]
  public?: boolean
  pricing?: {
    inputPerMillionUsd: string
    outputPerMillionUsd: string
    cacheReadInputPerMillionUsd?: string
    cacheWriteInputPerMillionUsd?: string
    [key: string]: unknown
  }
  [key: string]: unknown
}

export interface AiBackendSpec {
  provider: AiProvider
  endpoint?: string
  allowInsecureHttp?: boolean
  credentialPool: {
    credentials: AiCredential[]
    redisRef?: string
    onRedisFailure?: 'Deny' | 'Allow'
    [key: string]: unknown
  }
  models: AiModel[]
  defaults?: { streaming?: boolean; maxOutputTokens?: number; [key: string]: unknown }
  [key: string]: unknown
}

export interface EdgionBackend {
  apiVersion: string
  kind: 'EdgionBackend'
  metadata: K8sMetadata
  spec: { ai: AiBackendSpec; [key: string]: unknown }
  status?: unknown
}
