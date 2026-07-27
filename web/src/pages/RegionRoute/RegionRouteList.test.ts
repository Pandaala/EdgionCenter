import { describe, expect, it } from 'vitest'
import type { EffectiveRegionRoute } from '@/api/regionRoute'
import { regionRouteRowKey, writableOverrideRef } from './RegionRouteList'

function effective(entryIndex: number, permitted: boolean): EffectiveRegionRoute {
  return {
    namespace: 'shop',
    pluginName: 'regional',
    alias: 'duplicate',
    entryIndex,
    myRegion: 'east',
    regions: [],
    keyGet: [],
    routeRules: [],
    overrideRef: { namespace: 'shop', name: `override-${entryIndex}`, permitted },
    overrideApplied: false,
    serviceUsages: [],
  }
}

describe('RegionRoute row identity and writable references', () => {
  it('keeps duplicate aliases distinct', () => {
    expect(regionRouteRowKey(effective(0, true))).not.toBe(regionRouteRowKey(effective(1, true)))
  })

  it('rejects a denied override reference', () => {
    expect(writableOverrideRef(effective(0, false))).toBeNull()
    expect(writableOverrideRef(effective(0, true))?.name).toBe('override-0')
  })

  it('reports no writable reference when the route has none', () => {
    const route = effective(0, true)
    route.overrideRef = null
    expect(writableOverrideRef(route)).toBeNull()
  })
})
