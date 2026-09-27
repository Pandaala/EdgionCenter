import type { RedisConfig } from '@/types/link-sys'
import { parseGep2257DurationMilliseconds } from './validation'

export function validateRedis(config: RedisConfig): void {
  const fail = (message: string): never => { throw new Error(`Redis ${message}`) }
  const topology = config.topology
  const mode = topology?.mode ?? 'standalone'
  let endpoints = config.endpoints ?? []
  if (mode === 'standalone') {
    if (endpoints.length !== 1) fail('standalone mode requires exactly one endpoint')
    if (topology?.sentinel || topology?.cluster) fail('standalone mode must not configure sentinel or cluster settings')
  } else if (mode === 'sentinel') {
    if (endpoints.length) fail('sentinel mode must not configure top-level endpoints')
    if (topology?.cluster) fail('sentinel mode must not configure cluster settings')
    if (!topology?.sentinel?.masterName?.trim() || !topology.sentinel.sentinels?.length) fail('sentinel mode requires a master name and sentinel endpoints')
    endpoints = topology!.sentinel!.sentinels
  } else if (mode === 'cluster') {
    if (!endpoints.length) fail('cluster mode requires at least one endpoint')
    if (topology?.sentinel) fail('cluster mode must not configure sentinel settings')
  } else fail('topology mode is invalid')
  for (const [index, endpoint] of endpoints.entries()) {
    try {
      if (typeof endpoint !== 'string' || !endpoint || endpoint.trim() !== endpoint) throw new Error()
      const authority = endpoint.replace(/^rediss?:\/\//, '')
      if (authority.includes('://') || authority.split(/[/?#]/)[0].endsWith(':') || /[?#]/.test(authority)) throw new Error()
      const slash = authority.indexOf('/')
      if (slash >= 0 && authority.slice(slash) !== '/') throw new Error()
      const url = new URL(`redis://${authority}`)
      if (!url.hostname || url.username || url.password || !['', '/'].includes(url.pathname) || url.port === '0') throw new Error()
    } catch { fail(`active endpoint[${index}] must be a Redis origin without userinfo, path, query, or fragment`) }
  }
  if (!config.tls?.enabled && endpoints.some((value) => value.startsWith('rediss://')) && endpoints.some((value) => !value.startsWith('rediss://'))) fail('active endpoints cannot mix TLS and plaintext unless tls.enabled=true')
  if (config.auth && !config.auth.secretRef?.name) fail('auth requires a Secret name')
  const db = config.db ?? 0
  if (!Number.isInteger(db) || db < 0 || db > 255) fail('db must be an integer from 0 to 255')
  const poolSize = config.pool?.size
  if (poolSize != null && (!Number.isInteger(poolSize) || poolSize < 1 || poolSize > 64)) fail('pool.size must be an integer from 1 to 64')
  for (const [field, max] of [['connect', 10000], ['command', 30000]] as const) {
    const raw = config.timeout?.[field]
    if (raw == null) continue
    const value = typeof raw === 'string' ? parseGep2257DurationMilliseconds(raw) : null
    if (value === null || value <= 0 || value > max) fail(`timeout.${field} must be a positive GEP-2257 duration no greater than ${max / 1000}s`)
  }
  const redirects = topology?.cluster?.maxRedirects
  if (redirects != null && (!Number.isInteger(redirects) || redirects < 0 || redirects > 64)) fail('topology.cluster.maxRedirects must be an integer from 0 to 64')
  const raw = config as any
  if (raw.retry !== undefined || raw.observability !== undefined || raw.pool?.minIdle !== undefined
    || raw.timeout?.read !== undefined || raw.timeout?.write !== undefined || raw.topology?.cluster?.readFromReplicas !== undefined) fail('contains removed retry, observability, pool.minIdle, timeout.read/write, or readFromReplicas fields')
}
