import { describe, expect, it } from 'vitest'
import type {
  CenterRegionRouteOverride,
  RegionRouteOverrideResource,
  WriteOutcomeSummary,
} from '@/api/regionRoute'
import {
  describeObservedRegions,
  flattenRegionOutcomes,
  overrideConsistent,
  overrideMatchesFilters,
} from './RegionRouteOverridePage'

function resource(failoverTo?: string): RegionRouteOverrideResource {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'EdgionConfigData',
    metadata: { namespace: 'shop', name: 'checkout' },
    spec: {
      data: {
        type: 'ServiceRegionRouteOverride',
        config: {
          regions: [{ name: 'east', failoverTo }],
        },
      },
    },
  }
}

describe('RegionRoute override consistency', () => {
  it('requires the resource on every online Controller', () => {
    const row: CenterRegionRouteOverride = {
      namespace: 'shop',
      name: 'checkout',
      controllers: { first: resource('west') },
    }
    expect(overrideConsistent(row, ['first', 'second'])).toBe(false)
  })

  it('compares only the managed spec rather than server metadata', () => {
    const first = resource('west')
    const second = resource('west')
    first.metadata.resourceVersion = '1'
    second.metadata.resourceVersion = '99'
    const row: CenterRegionRouteOverride = {
      namespace: 'shop',
      name: 'checkout',
      controllers: { first, second },
    }
    expect(overrideConsistent(row, ['first', 'second'])).toBe(true)
  })
})

describe('RegionRoute override filters', () => {
  const row: CenterRegionRouteOverride = {
    namespace: 'shop-production',
    name: 'checkout-route',
    controllers: {},
  }

  it('filters namespace and name independently', () => {
    expect(overrideMatchesFilters(row, 'production', '')).toBe(true)
    expect(overrideMatchesFilters(row, '', 'checkout')).toBe(true)
    expect(overrideMatchesFilters(row, 'other', '')).toBe(false)
    expect(overrideMatchesFilters(row, '', 'payment')).toBe(false)
  })

  it('combines namespace and name filters', () => {
    expect(overrideMatchesFilters(row, 'SHOP', 'ROUTE')).toBe(true)
    expect(overrideMatchesFilters(row, 'shop', 'payment')).toBe(false)
  })
})

describe('flattenRegionOutcomes', () => {
  const summary = (): WriteOutcomeSummary => ({
    modified: 1,
    failed: 1,
    outcomes: [
      { controllerId: 'ctrl-a', state: 'converged' },
      { controllerId: 'ctrl-b', state: 'failed', reason: 'not in the local watch cache' },
    ],
  })

  it('produces one labeled row per (region, controller) outcome, across every applied region', () => {
    const items = flattenRegionOutcomes([
      { region: 'east', summary: summary() },
      { region: 'west', summary: summary() },
    ])
    expect(items).toHaveLength(4)
    expect(items.map((item) => item.key)).toEqual([
      'east:ctrl-a', 'east:ctrl-b', 'west:ctrl-a', 'west:ctrl-b',
    ])
    expect(items[0].label).toContain('east')
    expect(items[0].label).toContain('ctrl-a')
    expect(items[1].outcome.state).toBe('failed')
  })

  it('returns an empty list for no applied regions', () => {
    expect(flattenRegionOutcomes([])).toEqual([])
  })
})

describe('describeObservedRegions', () => {
  it('summarizes every region\'s current failoverTo from the observed document', () => {
    const observed = {
      spec: { data: { config: { regions: [
        { name: 'east', failoverTo: 'west' },
        { name: 'west' },
      ] } } },
    }
    const text = describeObservedRegions(observed)
    expect(text).toContain('east')
    expect(text).toContain('west')
  })

  it('returns an empty string for an unrecognized or missing observed shape', () => {
    expect(describeObservedRegions(undefined)).toBe('')
    expect(describeObservedRegions({})).toBe('')
    expect(describeObservedRegions({ spec: {} })).toBe('')
  })
})
