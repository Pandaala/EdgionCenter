import { describe, expect, it } from 'vitest'
import type { CenterRegionRoute, EffectiveRegionRoute } from '@/api/regionRoute'
import {
  regionRouteConsistencyKey,
  regionRouteRowKey,
  writableOverrideRef,
} from './RegionRouteList'

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
  it('keeps duplicate aliases distinct and matches the backend consistency name', () => {
    const first = effective(0, true)
    const second = effective(1, true)
    expect(regionRouteRowKey(first)).not.toBe(regionRouteRowKey(second))
    expect(regionRouteConsistencyKey(first)).toBe('shop/regional/duplicate (#0)')
    expect(regionRouteConsistencyKey(second)).toBe('shop/regional/duplicate (#1)')
  })

  it('rejects denied references in controller and Center views', () => {
    expect(writableOverrideRef(effective(0, false))).toBeNull()
    const center: CenterRegionRoute = {
      namespace: 'shop',
      pluginName: 'regional',
      alias: null,
      entryIndex: 0,
      controllers: {
        denied: effective(0, false),
        permitted: effective(0, true),
      },
    }
    expect(writableOverrideRef(center)?.name).toBe('override-0')
    center.controllers.permitted.overrideRef = null
    expect(writableOverrideRef(center)).toBeNull()
  })
})
