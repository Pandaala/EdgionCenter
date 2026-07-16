import { describe, expect, it } from 'vitest'
import { normalize, toMutationDocument } from './edgionstreamplugins'

describe('EdgionStreamPlugins lossless adapter', () => {
  it('preserves both stages, all current variants, unknown fields, and explicit empties', () => {
    const fixture = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionStreamPlugins' as const,
      metadata: { name: 'stream', namespace: 'edge', labels: { team: 'net' }, resourceVersion: '7' },
      spec: {
        plugins: [
          { enable: true, type: 'IpRestriction', config: { allow: [{ name: 'office', description: '', cidrs: ['10.0.0.0/8'] }], defaultAction: 'deny', future: false, allowMatcher: 'runtime' } },
          { enable: false, type: 'GlobalConnectionIpRestriction', config: { enable: true, activeProfile: 'safe', profiles: { safe: { allow: [{ name: 'loopback', cidrs: ['127.0.0.1'] }], defaultAction: 'deny', denyMatcher: 'runtime' } } } },
          { type: 'ConnectionRateLimit', config: { redisRef: 'edge/redis', perSourceIp: { rate: 2, interval: '1s', intervalDuration: 1000 }, futureRateField: [] } },
        ],
        tlsRoutePlugins: [{ type: 'IpRestriction', config: { ipSource: 'remoteAddr', status: 403, allow: [{ name: 'tls', cidrs: ['192.0.2.0/24'] }], ipMatcher: 'runtime', futureTls: true } }],
        futureSpec: { enabled: true },
      },
      status: { conditions: [] },
    }

    const view = normalize(fixture)
    const mutation = toMutationDocument(view, 'update')

    expect(view).toEqual(fixture)
    expect(mutation).toHaveProperty('spec.plugins.0.config.allow.0.name', 'office')
    expect(mutation).toHaveProperty('spec.plugins.1.config.profiles.safe.defaultAction', 'deny')
    expect(mutation).toHaveProperty('spec.plugins.2.config.futureRateField', [])
    expect(mutation).toHaveProperty('spec.tlsRoutePlugins.0.config.futureTls', true)
    expect(mutation).not.toHaveProperty('spec.plugins.0.config.allowMatcher')
    expect(mutation).not.toHaveProperty('spec.plugins.1.config.profiles.safe.denyMatcher')
    expect(mutation).not.toHaveProperty('spec.plugins.2.config.perSourceIp.intervalDuration')
    expect(mutation).not.toHaveProperty('spec.tlsRoutePlugins.0.config.ipMatcher')
    expect(mutation).not.toHaveProperty('status')
    expect(mutation).toHaveProperty('metadata.resourceVersion', '7')
  })
})
