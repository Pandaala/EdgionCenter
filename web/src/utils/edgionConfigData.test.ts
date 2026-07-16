import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import {
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

describe('EdgionConfigData resource adapter', () => {
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
