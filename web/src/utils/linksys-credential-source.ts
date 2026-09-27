import type { CredentialSourceConfig, LinkSysConfig, SecretObjectReference } from '@/types/link-sys'
import { parseGep2257DurationMilliseconds } from './validation'

export function validateCredentialSource(value: LinkSysConfig): void {
  const config = value as CredentialSourceConfig
  const fail = (message: string): never => { throw new Error(`credentialSource.${message}`) }
  const provider = config.provider
  if (provider?.type !== 'oauth2ClientCredentials') fail('provider.type must be oauth2ClientCredentials')
  let endpoint: URL
  try { endpoint = new URL(provider.tokenEndpoint) } catch { fail('provider.tokenEndpoint must be a valid HTTPS URL') }
  if (typeof provider.tokenEndpoint !== 'string' || new TextEncoder().encode(provider.tokenEndpoint).length > 2048
    || endpoint!.protocol !== 'https:' || !endpoint!.hostname || endpoint!.username || endpoint!.password
    || provider.tokenEndpoint.includes('?') || provider.tokenEndpoint.includes('#')) fail('provider.tokenEndpoint must be HTTPS without userinfo, query, or fragment')
  const auth = provider.clientAuthentication
  if (!auth) fail('provider.clientAuthentication is required')
  if (auth.method !== undefined && auth.method !== 'clientSecretBasic') fail('provider.clientAuthentication.method must be clientSecretBasic')
  const checkRef = (ref: SecretObjectReference | undefined, field: string) => {
    if (!ref?.name || (ref.group != null && !['', 'core'].includes(ref.group)) || (ref.kind != null && ref.kind !== 'Secret')) fail(`provider.clientAuthentication.${field} must reference a core Secret`)
  }
  checkRef(auth.activeSecretRef, 'activeSecretRef')
  if (auth.previousSecretRef != null) {
    checkRef(auth.previousSecretRef, 'previousSecretRef')
    if (['group', 'kind', 'namespace', 'name'].every((key) => (auth.activeSecretRef as any)[key] === (auth.previousSecretRef as any)[key])) fail('provider.clientAuthentication.previousSecretRef must differ from activeSecretRef')
  }
  const scopes = provider.scopes ?? []
  if (!Array.isArray(scopes) || scopes.length > 16 || new Set(scopes).size !== scopes.length
    || scopes.some((scope) => typeof scope !== 'string' || !/^[\x21\x23-\x5b\x5d-\x7e]{1,256}$/.test(scope))
    || scopes.join(' ').length > 2048) fail('provider.scopes must contain at most 16 unique RFC 6749 scope tokens, each at most 256 bytes and together at most 2048 bytes')
  const limits = [
    ['interval', '15m', 30000, 86400000],
    ['requestTimeout', '10s', 1000, 60000],
    ['retryInitialBackoff', '2s', 1000, 60000],
    ['retryMaxBackoff', '2m', 1000, 900000],
  ] as const
  const durations: Record<string, number> = {}
  for (const [field, fallback, min, max] of limits) {
    const raw = config.rotation?.[field] ?? fallback
    const duration = typeof raw === 'string' ? parseGep2257DurationMilliseconds(raw) : null
    if (duration === null || duration < min || duration > max) fail(`rotation.${field} must be a GEP-2257 duration between ${min / 1000}s and ${max / 1000}s`)
    durations[field] = duration!
  }
  if (durations.retryInitialBackoff > durations.retryMaxBackoff) fail('rotation.retryInitialBackoff must not exceed retryMaxBackoff')
  const maxKeys = config.publication?.memoryMaxKeys ?? 10000
  if (!Number.isInteger(maxKeys) || maxKeys < 1 || maxKeys > 10000) fail('publication.memoryMaxKeys must be an integer from 1 to 10000')
}
