import * as yaml from 'js-yaml'
import type { AcmeChallenge, ChallengeType, EdgionAcme } from '@/types/edgion-acme'
import { buildMutationDocument, withCreateDefaults } from './resource-document'
import { isValidGep2257Duration } from './validation'
import { dumpYaml } from './yaml-utils'

export const DEFAULT_YAML = `apiVersion: edgion.io/v1
kind: EdgionAcme
metadata:
  name: lets-encrypt
  namespace: default
spec:
  email: admin@example.com
  privateKeySecretRef:
    name: acme-account
  domains:
    - example.com
  keyType: ecdsa-p256
  challenge:
    type: http-01
    gatewayRef:
      name: my-gateway
  storage:
    secretName: acme-cert
  renewal:
    renewBefore: 720h
    checkInterval: 24h
    failBackoff: 5m
  autoEdgionTls:
    enabled: true
`

const CREATE_DEFAULTS: EdgionAcme = {
  apiVersion: 'edgion.io/v1',
  kind: 'EdgionAcme',
  metadata: { name: '', namespace: 'default' },
  spec: {
    server: 'https://acme-v02.api.letsencrypt.org/directory',
    email: '',
    privateKeySecretRef: { name: '' },
    domains: [],
    keyType: 'ecdsa-p256',
    challenge: { type: 'http-01', gatewayRef: { name: '' } },
    storage: { secretName: '' },
    renewal: { renewBefore: '720h', checkInterval: '24h', failBackoff: '5m' },
    autoEdgionTls: { enabled: true },
  },
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

export function createEmpty(): EdgionAcme {
  return withCreateDefaults(undefined, CREATE_DEFAULTS)
}

/** Validate the resource identity and retain empty, false, and unknown fields. */
export function normalize(raw: unknown): EdgionAcme {
  if (!isObject(raw) || raw.kind !== 'EdgionAcme' || !isObject(raw.metadata) || !isObject(raw.spec)) {
    throw new Error('YAML must contain an EdgionAcme resource with metadata and spec')
  }
  if (!isObject(raw.spec.challenge) || !['http-01', 'dns-01'].includes(String(raw.spec.challenge.type))) {
    throw new Error('EdgionAcme challenge.type must be http-01 or dns-01')
  }
  if (raw.spec.challenge.type === 'http-01' && !isObject(raw.spec.challenge.gatewayRef)) {
    throw new Error('EdgionAcme HTTP-01 challenge requires flat challenge.gatewayRef')
  }
  if (raw.spec.challenge.type === 'dns-01'
    && (typeof raw.spec.challenge.provider !== 'string' || !isObject(raw.spec.challenge.credentialRef))) {
    throw new Error('EdgionAcme DNS-01 challenge requires flat provider and credentialRef')
  }
  return raw as unknown as EdgionAcme
}

export function validateEdgionAcme(resource: EdgionAcme): string[] {
  const errors: string[] = []
  const durations: Array<[string, unknown]> = [
    ['challenge.propagationTimeout',
      resource.spec.challenge.type === 'dns-01'
        ? resource.spec.challenge.propagationTimeout
        : undefined],
    ['challenge.propagationCheckInterval',
      resource.spec.challenge.type === 'dns-01'
        ? resource.spec.challenge.propagationCheckInterval
        : undefined],
    ['renewal.renewBefore', resource.spec.renewal?.renewBefore],
    ['renewal.checkInterval', resource.spec.renewal?.checkInterval],
    ['renewal.failBackoff', resource.spec.renewal?.failBackoff],
  ]
  durations.forEach(([path, value]) => {
    if (value !== undefined
      && (typeof value !== 'string' || !isValidGep2257Duration(value))) {
      errors.push(`${path} must be a valid GEP-2257 duration`)
    }
  })
  return errors
}

export function toMutationDocument(
  resource: EdgionAcme,
  mode: 'create' | 'update',
): Record<string, unknown> {
  const validationErrors = validateEdgionAcme(resource)
  if (validationErrors.length > 0) {
    throw new Error(validationErrors.join('; '))
  }
  return buildMutationDocument(resource, { resourceKind: 'edgionacme', mode })
}

export function toYaml(resource: EdgionAcme, mode: 'create' | 'update' = 'update'): string {
  return dumpYaml(toMutationDocument(resource, mode))
}

export function fromYaml(yamlStr: string): EdgionAcme {
  return normalize(yaml.load(yamlStr))
}

/** Switch the tagged union, removing only fields owned by the previous variant. */
export function replaceChallengeType(challenge: AcmeChallenge, type: ChallengeType): AcmeChallenge {
  if (challenge.type === type) return challenge
  if (type === 'http-01') {
    const unknown: Record<string, unknown> = { ...challenge }
    delete unknown.provider
    delete unknown.credentialRef
    delete unknown.propagationTimeout
    delete unknown.propagationCheckInterval
    return { ...unknown, type, gatewayRef: { name: '' } }
  }
  const unknown: Record<string, unknown> = { ...challenge }
  delete unknown.gatewayRef
  return {
    ...unknown,
    type,
    provider: '',
    credentialRef: { name: '' },
    propagationTimeout: '120s',
    propagationCheckInterval: '5s',
  }
}
