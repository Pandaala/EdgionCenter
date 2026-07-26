import { describe, expect, it } from 'vitest'
import type {
  CenterRegionRouteOverride,
  RegionRouteOverrideResource,
} from '@/api/regionRoute'
import {
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
