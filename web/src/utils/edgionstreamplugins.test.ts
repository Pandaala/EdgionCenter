import { describe, expect, it } from 'vitest'
import { fromYaml, normalize, toMutationDocument } from './edgionstreamplugins'

describe('EdgionStreamPlugins lossless adapter', () => {
  it.each(['create', 'update'] as const)('retains profile names matching internal fields on %s', (mode) => {
    const profile = { allowRefs: [{ name: 'office', refDenied: { reason: 'RefNotPermitted' } }], defaultAction: 'deny', allowMatcher: 'runtime' }
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionStreamPlugins', metadata: { name: 'named', namespace: 'edge' },
      spec: { plugins: [{ type: 'GlobalConnectionIpRestriction', config: {
        enable: true, activeProfile: 'allowMatcher', activeProfileRef: { name: 'selector', refDenied: {} },
        profiles: { allowMatcher: structuredClone(profile), refDenied: structuredClone(profile) },
        future: { refDenied: 'operator data' },
      } }] },
    }
    const before = structuredClone(resource)
    const mutation: any = toMutationDocument(resource, mode)
    expect(mutation.spec.plugins[0].config).toEqual({
      enable: true, activeProfile: 'allowMatcher', activeProfileRef: { name: 'selector' },
      profiles: {
        allowMatcher: { allowRefs: [{ name: 'office' }], defaultAction: 'deny' },
        refDenied: { allowRefs: [{ name: 'office' }], defaultAction: 'deny' },
      },
      future: { refDenied: 'operator data' },
    })
    expect(resource).toEqual(before)
  })

  it('preserves both stages, all current variants, unknown fields, and explicit empties', () => {
    const fixture = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionStreamPlugins' as const,
      metadata: { name: 'stream', namespace: 'edge', labels: { team: 'net' }, resourceVersion: '7' },
      spec: {
        plugins: [
          { enable: true, type: 'IpRestriction', config: { allow: [{ name: 'office', description: '', cidrs: ['10.0.0.0/8'] }], defaultAction: 'deny', future: false, allowMatcher: 'runtime' } },
          { enable: false, type: 'GlobalConnectionIpRestriction', config: { enable: true, activeProfile: 'safe', profiles: { safe: { allow: [{ name: 'loopback', cidrs: ['127.0.0.1'] }], defaultAction: 'deny', denyMatcher: 'runtime' } } } },
          { type: 'GeoIpLocation', config: { defaultAction: 'allow', failOpen: false, denyRuleGroups: [], futureGeo: 0 } },
          { type: 'ConnectionRateLimit', config: { redisRef: 'edge/redis', perSourceIp: { rate: 2, interval: '1s', intervalDuration: 1000 }, futureRateField: [] } },
        ],
        tlsRoutePlugins: [{ type: 'IpRestriction', config: { ipSource: 'RemoteIp', status: 403, allow: [{ name: 'tls', cidrs: ['192.0.2.0/24'] }], ipMatcher: 'runtime', futureTls: true } }],
        futureSpec: { enabled: true },
      },
      status: { conditions: [] },
    }

    const view = normalize(fixture)
    const mutation = toMutationDocument(view, 'update')

    expect(view).toEqual(fixture)
    expect(mutation).toHaveProperty('spec.plugins.0.config.allow.0.name', 'office')
    expect(mutation).toHaveProperty('spec.plugins.1.config.profiles.safe.defaultAction', 'deny')
    expect(mutation).toHaveProperty('spec.plugins.3.config.futureRateField', [])
    expect(mutation).toHaveProperty('spec.plugins.2.config.futureGeo', 0)
    expect(mutation).toHaveProperty('spec.tlsRoutePlugins.0.config.futureTls', true)
    expect(mutation).not.toHaveProperty('spec.plugins.0.config.allowMatcher')
    expect(mutation).not.toHaveProperty('spec.plugins.1.config.profiles.safe.denyMatcher')
    expect(mutation).not.toHaveProperty('spec.plugins.3.config.perSourceIp.intervalDuration')
    expect(mutation).not.toHaveProperty('spec.tlsRoutePlugins.0.config.ipMatcher')
    expect(mutation).not.toHaveProperty('status')
    expect(mutation).toHaveProperty('metadata.resourceVersion', '7')
  })
})


describe('stream plugin YAML structure', () => {
  it.each(['plugins', 'tlsRoutePlugins'])('validates %s while retaining unknown plugin config', (stage) => {
    const document = { kind: 'EdgionStreamPlugins', metadata: { name: 'stream' }, spec: { [stage]: [{ type: 'FuturePlugin', config: { future: false } }] } }
    expect(fromYaml(JSON.stringify(document))).toEqual(document)
    for (const entries of [{}, [null], ['IpRestriction'], [{ type: 'IpRestriction', config: [] }]]) {
      expect(() => fromYaml(JSON.stringify({ ...document, spec: { [stage]: entries } }))).toThrow(stage)
    }
    expect(fromYaml(JSON.stringify({ ...document, spec: { [stage]: null } })).spec).toEqual({ [stage]: null })
  })

  it('rejects array metadata and spec before rendering', () => {
    expect(() => normalize({ kind: 'EdgionStreamPlugins', metadata: [], spec: {} })).toThrow('metadata and spec')
    expect(() => normalize({ kind: 'EdgionStreamPlugins', metadata: {}, spec: [] })).toThrow('metadata and spec')
  })
})
