import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import {
  CONFIG_DATA_TYPES,
  fromYaml,
  normalize,
  replaceConfigDataType,
  toMutationDocument,
  toMutationYaml,
  toYaml,
  type EdgionConfigDataResource,
} from './edgionConfigData'

const variantFixtures: EdgionConfigDataResource[] = [
  {
    apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData',
    metadata: { name: 'keys', namespace: 'edge' },
    spec: { enable: false, visibility: 'Namespace', active: '', data: {
      type: 'KeyList', config: { matchMode: 'regex', items: [{ name: 'a', description: '', items: [{ key: '^x$', code: 0 }] }] },
    } },
  },
  {
    apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData',
    metadata: { name: 'ips', namespace: 'edge' },
    spec: { enable: true, visibility: 'Cluster', data: {
      type: 'IpList', config: { items: [{ name: 'corp', description: '', cidrs: [] }] },
    } },
  },
  {
    apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData',
    metadata: { name: 'selector', namespace: 'edge' },
    spec: { enable: true, visibility: 'Namespace', data: {
      type: 'Selector', config: { active: '', description: '', futureSelectorField: false },
    } },
  },
  {
    apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData',
    metadata: { name: 'regions', namespace: 'edge' },
    spec: { enable: true, visibility: 'Namespace', data: {
      type: 'RegionRouteOverride', config: {
        enable: false, active: '', regions: [{ name: 'zero', hashRange: [0, 0], backendEndpoint: '127.0.0.1:80', tls: false }],
      },
    } },
  },
  {
    apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData',
    metadata: { name: 'misc', namespace: 'edge' },
    spec: { enable: true, visibility: 'Namespace', data: {
      type: 'Misc', config: { unknown: { empty: [], disabled: false, count: 0, text: '' } },
    }, futureSpecField: { enabled: false } },
  },
]

const newVariants = [
  { type: 'RequestAccessUrlAllowList', config: { items: [{ name: 'public', paths: [{ type: 'Exact', value: '/health' }] }] } },
  { type: 'ProxyProtocolTrust', config: { mode: 'trustedSources', trustedCidrs: ['192.0.2.0/24'] } },
  { type: 'WafRuleBundle', config: { version: '1', profile: 'local', provenance: 'operator', roots: ['main.conf'], rules: [{ name: 'main.conf', content: 'SecRuleEngine On' }], phraseAssets: [] } },
  { type: 'WafPolicy', config: { defaultProfile: 'main', profiles: { main: { bundleRefs: [{ name: 'rules', optional: false }] } } } },
] as const
for (const data of newVariants) {
  variantFixtures.push({ ...variantFixtures[0], spec: { ...variantFixtures[0].spec, data: structuredClone(data) } })
}

describe('EdgionConfigData resource adapter', () => {
  it('covers every current Controller variant in lossless round trips', () => {
    expect(variantFixtures.map((fixture) => fixture.spec.data.type).sort()).toEqual([...CONFIG_DATA_TYPES].sort())
    for (const fixture of variantFixtures) {
      expect(toMutationDocument(fromYaml(toYaml(fixture)), 'update').spec).toEqual(fixture.spec)
    }
  })
  it.each(variantFixtures.map((fixture) => [fixture.spec.data.type, fixture] as const))(
    'round-trips the complete %s fixture',
    (_type, fixture) => {
      const parsed = fromYaml(toYaml(fixture))
      expect(parsed).toEqual(fixture)
    },
  )

  it('preserves empty, false, zero and unknown operator fields while stripping server fields', () => {
    const raw = {
      ...variantFixtures[4],
      metadata: {
        ...variantFixtures[4].metadata,
        uid: 'server-uid', resourceVersion: '12', managedFields: [{ manager: 'controller' }],
        labels: { empty: '' },
      },
      status: { conditions: [] },
      serverExtension: true,
    }
    expect(normalize(raw)).toBe(raw)
    expect(fromYaml(toYaml(raw))).toEqual(raw)
    expect(yaml.load(toMutationYaml(raw, 'update'))).toEqual(toMutationDocument(raw, 'update'))
    expect(toMutationDocument(raw, 'update')).toEqual({
      apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData',
      metadata: { name: 'misc', namespace: 'edge', labels: { empty: '' }, resourceVersion: '12' },
      spec: raw.spec,
    })
  })

  it('omits resolved URL condition data without stripping arbitrary Misc fields', () => {
    const resource = structuredClone(variantFixtures.find((value) => value.spec.data.type === 'RequestAccessUrlAllowList')!)
    resource.spec.data.config = { items: [{ name: 'signed', paths: [{ type: 'Exact', value: '/' }], conditions: { allOf: [
      { type: 'secretMatch', key: { type: 'header', name: 'Authorization' }, secretRef: { name: 'auth' }, secretKey: 'token', resolvedValues: ['runtime-value'] },
      { type: 'hmacAuth', resolvedCredentials: { user: { secret: [1, 2] } }, secretGroups: [] },
    ] } }] }
    const output = toMutationDocument(resource, 'update')
    expect(JSON.stringify(output)).not.toContain('resolvedValues')
    expect(JSON.stringify(output)).not.toContain('resolvedCredentials')
    expect(JSON.stringify(output)).toContain('secretRef')
    resource.spec.data.type = 'Misc'
    expect(toMutationDocument(resource, 'update').spec).toEqual(resource.spec)
  })

  it('changes only the tagged entry when selecting another variant', () => {
    const fixture = { ...variantFixtures[4], customTopLevel: { retained: true } }
    const changed = replaceConfigDataType(fixture, 'Selector')
    expect(changed.spec.data).toEqual({ type: 'Selector', config: {} })
    expect(changed.spec.futureSpecField).toEqual({ enabled: false })
    expect(changed.customTopLevel).toEqual({ retained: true })
  })

  it('rejects a payload whose discriminator envelope is missing', () => {
    const invalid = yaml.dump({ apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData', metadata: {}, spec: {} })
    expect(() => fromYaml(invalid)).toThrow(/spec.data/)
  })
})
