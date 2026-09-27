import { describe, expect, it } from 'vitest'
import { validateRedis } from './linksys-redis'
import { createEmpty, validateLinkSys } from './linksys'

describe('current Redis LinkSys contract', () => {
  it('accepts standalone, optional cluster settings and Sentinel-only endpoints', () => {
    for (const config of [
      { endpoints: ['redis://cache:6379'], db: 255, timeout: { connect: '10s', command: '30s' }, pool: { size: 64 } },
      { endpoints: ['rediss://one:6379', 'rediss://two:6379'], topology: { mode: 'cluster' as const } },
      { endpoints: [], topology: { mode: 'sentinel' as const, sentinel: { masterName: 'primary', sentinels: ['sentinel:26379'] } } },
    ]) expect(() => validateRedis(config)).not.toThrow()
  })

  it('rejects invalid topology, credentials in URLs and mixed transport', () => {
    for (const config of [
      { endpoints: ['one:6379', 'two:6379'] },
      { endpoints: ['redis://user:pass@cache:6379'] },
      { endpoints: ['redis://cache:6379/1'] },
      { endpoints: ['cache:0'] },
      { endpoints: ['cache:6379?'] },
      { endpoints: ['redis://one:6379', 'rediss://two:6379'], topology: { mode: 'cluster' } },
      { endpoints: ['cache:6379'], topology: { mode: 'sentinel', sentinel: { masterName: 'primary', sentinels: ['sentinel:26379'] } } },
    ]) expect(() => validateRedis(config as any)).toThrow('Redis')
  })

  it('validates durations and bounds and refuses removed controls without dropping them', () => {
    for (const patch of [{ db: 256 }, { db: 0.5 }, { pool: { size: 65 } }, { timeout: { connect: 5000 } }, { timeout: { command: '31s' } }, { timeout: { connect: '0s' } }, { timeout: { read: 10 } }, { retry: {} }, { observability: {} }, { topology: { mode: 'cluster', cluster: { maxRedirects: 65 } } }]) {
      const config = { endpoints: ['cache:6379'], ...patch }
      expect(() => validateRedis(config as any)).toThrow('Redis')
      expect(config).toMatchObject(patch)
    }
  })
})

describe('Kafka queue limits', () => {
  it('validates all independent capacity limits while preserving zero linger', () => {
    const resource = createEmpty()
    resource.spec = { type: 'kafka', config: { brokers: ['broker:9092'], channelSize: 1, maxTopics: 1, maxPendingRecords: 1, maxPendingBytes: 1, lingerMs: 0 } }
    expect(() => validateLinkSys(resource)).not.toThrow()
    for (const field of ['channelSize', 'maxTopics', 'maxPendingRecords', 'maxPendingBytes']) {
      const invalid = structuredClone(resource)
      ;(invalid.spec.config as any)[field] = 0
      expect(() => validateLinkSys(invalid)).toThrow(field)
    }
  })
})
