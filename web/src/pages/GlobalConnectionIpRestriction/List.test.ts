import { describe, expect, it } from 'vitest'
import type { FanOutResponse } from '@/api/globalConnectionIpRestriction'
import type { WriteOutcome } from '@/api/writeOutcome'
import { describeSelectorActiveProfile, resolveWriteOutcome } from './List'

function fanOut(success: FanOutResponse['success'], failed: FanOutResponse['failed']): FanOutResponse {
  return { success, failed, warnings: [] }
}

function outcome(overrides: Partial<WriteOutcome>): WriteOutcome {
  return { controllerId: 'ctrl-a', state: 'converged', ...overrides }
}

describe('resolveWriteOutcome', () => {
  it('finds the outcome for a controller listed under success', () => {
    const response = fanOut(
      [{ controllerId: 'ctrl-a', outcome: outcome({ state: 'converged' }) }],
      [],
    )
    expect(resolveWriteOutcome(response, 'ctrl-a')?.state).toBe('converged')
  })

  it('finds the outcome for a controller listed under failed', () => {
    const response = fanOut(
      [],
      [{ controllerId: 'ctrl-b', error: 'boom', outcome: outcome({ controllerId: 'ctrl-b', state: 'conflict' }) }],
    )
    expect(resolveWriteOutcome(response, 'ctrl-b')?.state).toBe('conflict')
  })

  it('returns undefined when no result is attached for that controller', () => {
    const response = fanOut([], [])
    expect(resolveWriteOutcome(response, 'ctrl-a')).toBeUndefined()
  })

  it('returns undefined when the result exists but carries no outcome (pre-write-core Center)', () => {
    const response = fanOut([{ controllerId: 'ctrl-a', detail: 'ok' }], [])
    expect(resolveWriteOutcome(response, 'ctrl-a')).toBeUndefined()
  })
})

describe('describeSelectorActiveProfile', () => {
  it('extracts the active profile from an observed Selector document', () => {
    const observed = { spec: { data: { type: 'Selector', config: { active: 'strict' } } } }
    expect(describeSelectorActiveProfile(observed)).toBe('strict')
  })

  it('returns an empty string for an unrecognized or missing observed shape', () => {
    expect(describeSelectorActiveProfile(undefined)).toBe('')
    expect(describeSelectorActiveProfile({})).toBe('')
  })
})
