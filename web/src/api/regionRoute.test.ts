import { afterEach, describe, expect, it, vi } from 'vitest'
import { apiClient } from './client'
import { regionRouteApi } from './regionRoute'
import type { WriteOutcome } from './writeOutcome'

afterEach(() => {
  vi.restoreAllMocks()
})

function outcome(overrides: Partial<WriteOutcome>): WriteOutcome {
  return { controllerId: 'ctrl-a', state: 'converged', ...overrides }
}

describe('regionRouteApi.overrideFailover — RegionRoute failover (region_name form)', () => {
  it('returns the per-controller outcomes on a mixed 207 result instead of throwing', async () => {
    vi.spyOn(apiClient, 'post').mockResolvedValue({
      data: {
        success: false,
        data: {
          modified: 1,
          failed: 1,
          outcomes: [
            outcome({ controllerId: 'ctrl-a', state: 'converged' }),
            outcome({ controllerId: 'ctrl-b', state: 'failed', reason: 'not in the local watch cache' }),
          ],
        },
      },
    } as never)

    const result = await regionRouteApi.overrideFailover('region', 'ns', 'name', 'east', 'west')
    expect(result.modified).toBe(1)
    expect(result.failed).toBe(1)
    expect(result.outcomes).toHaveLength(2)
    expect(result.outcomes[1].state).toBe('failed')
  })

  it('recovers per-controller outcomes even when the request rejects (e.g. every target failed, 502)', async () => {
    vi.spyOn(apiClient, 'post').mockRejectedValue({
      response: {
        data: {
          success: false,
          data: {
            modified: 0,
            failed: 1,
            outcomes: [outcome({ state: 'conflict', reason: 'CAS precondition no longer matched' })],
          },
        },
      },
    })

    const result = await regionRouteApi.overrideFailover('region', 'ns', 'name', 'east', 'west')
    expect(result.failed).toBe(1)
    expect(result.outcomes[0].state).toBe('conflict')
  })

  it('rethrows when the backend never reached the write core (no outcomes to recover)', async () => {
    vi.spyOn(apiClient, 'post').mockRejectedValue({
      response: { data: { success: false, error: 'no online controllers' } },
    })

    await expect(
      regionRouteApi.overrideFailover('region', 'ns', 'name', 'east', 'west'),
    ).rejects.toBeTruthy()
  })
})

describe('regionRouteApi.syncOverride — row-level override sync', () => {
  it('reads per-controller detail from `outcomes`, not the retired `targets` field', async () => {
    // Regression guard: the backend field was renamed targets[] -> outcomes[].
    // A reader still keyed on `targets` degrades to a generic error message
    // and silently drops the per-controller detail.
    vi.spyOn(apiClient, 'post').mockResolvedValue({
      data: {
        success: false,
        data: {
          modified: 1,
          failed: 1,
          outcomes: [
            outcome({ controllerId: 'ctrl-a', state: 'converged' }),
            outcome({ controllerId: 'ctrl-b', state: 'failed', reason: 'not in the local watch cache' }),
          ],
        },
      },
    } as never)

    const result = await regionRouteApi.syncOverride('region', 'ns', 'name', 'ctrl-src', ['ctrl-a', 'ctrl-b'])
    expect(result.outcomes.map((o) => o.controllerId)).toEqual(['ctrl-a', 'ctrl-b'])
    const failedOutcome = result.outcomes.find((o) => o.controllerId === 'ctrl-b')
    expect(failedOutcome?.reason).toBe('not in the local watch cache')
  })

  it('does not silently swallow a legacy `targets`-shaped body into an empty outcome list', async () => {
    vi.spyOn(apiClient, 'post').mockResolvedValue({
      data: {
        success: false,
        data: {
          modified: 0,
          failed: 1,
          // Old field name — must not be misread as the new shape.
          targets: [{ controllerId: 'ctrl-b', success: false, error: 'boom' }],
        },
      },
    } as never)

    const result = await regionRouteApi.syncOverride('region', 'ns', 'name', 'ctrl-src', ['ctrl-b'])
    expect(result.outcomes).toEqual([])
    expect(result.failed).toBe(1)
  })
})
