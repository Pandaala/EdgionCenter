import * as yaml from 'js-yaml'
import type { EdgionBackend } from '@/types/edgion-backend'
import { dumpYaml } from './yaml-utils'
import { mutationDocumentToYaml } from './resource-document'

export function createEmptyEdgionBackend(): EdgionBackend {
  return {
    apiVersion: 'edgion.io/v1', kind: 'EdgionBackend',
    metadata: { name: '', namespace: 'default' },
    spec: { ai: {
      provider: 'OpenAI',
      credentialPool: { credentials: [{ name: 'primary', secretRef: { name: '' } }] },
      models: [{ name: '' }],
    } },
  }
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Validate editor shape without projecting away fields or injecting defaults. */
export function normalizeEdgionBackend(raw: unknown): EdgionBackend {
  if (!record(raw) || raw.kind !== 'EdgionBackend' || raw.apiVersion !== 'edgion.io/v1') {
    throw new Error('Expected an edgion.io/v1 EdgionBackend document')
  }
  if (!record(raw.metadata) || !record(raw.spec) || !record(raw.spec.ai)) {
    throw new Error('EdgionBackend requires metadata and spec.ai objects')
  }
  const ai = raw.spec.ai
  if (!record(ai.credentialPool) || !Array.isArray(ai.credentialPool.credentials)
    || !ai.credentialPool.credentials.every((entry) => record(entry) && record(entry.secretRef))
    || !Array.isArray(ai.models) || !ai.models.every(record)) {
    throw new Error('EdgionBackend requires credential references and models arrays')
  }
  return structuredClone(raw) as unknown as EdgionBackend
}

export const edgionBackendToYaml = (value: EdgionBackend): string => dumpYaml(value)
export const edgionBackendFromYaml = (source: string): EdgionBackend => normalizeEdgionBackend(yaml.load(source))
export const edgionBackendToMutationYaml = (value: EdgionBackend, mode: 'create' | 'update'): string =>
  mutationDocumentToYaml(value, 'edgionbackend', mode)

/** Required editor fields; the Controller remains authoritative for admission. */
export function validateEdgionBackend(value: EdgionBackend): string[] {
  const errors: string[] = []
  const ai = value.spec.ai
  if (!['OpenAI', 'OpenAICompatible', 'Anthropic'].includes(ai.provider)) errors.push('Unsupported AI provider')
  if (ai.provider === 'OpenAICompatible' && !ai.endpoint) errors.push('OpenAICompatible requires an endpoint')
  if (ai.endpoint) {
    try {
      const endpoint = new URL(ai.endpoint)
      if (endpoint.protocol !== 'https:' && !(endpoint.protocol === 'http:' && ai.allowInsecureHttp)) {
        errors.push('Endpoint must use HTTPS unless insecure HTTP is explicitly enabled')
      }
      if (endpoint.username || endpoint.password) errors.push('Endpoint must not contain credentials')
    } catch { errors.push('Endpoint must be an absolute URL') }
  }
  const credentials = ai.credentialPool.credentials
  if (credentials.length < 1 || credentials.length > 16) errors.push('Configure between 1 and 16 credentials')
  credentials.forEach((credential, index) => {
    if (!credential.name || !credential.secretRef.name) errors.push(`Credential ${index + 1} requires a name and Secret reference`)
    if (credential.weight !== undefined && (!Number.isInteger(credential.weight) || credential.weight < 1 || credential.weight > 1000000)) {
      errors.push(`Credential ${index + 1} weight must be between 1 and 1000000`)
    }
    if (credential.limits && (credential.limits.requestsPerMinute !== undefined || credential.limits.tokensPerMinute !== undefined)
      && !ai.credentialPool.redisRef) errors.push('Credential quotas require a Redis LinkSys reference')
  })
  if (ai.models.length < 1 || ai.models.length > 256) errors.push('Configure between 1 and 256 models')
  if (ai.models.some((model) => !model.name)) errors.push('Every model requires a name')
  return errors
}
