import { describe, expect, it } from 'vitest'
import type { ReloadOutcome } from '@/api/reloadOutcome'
import type { OutcomeState } from '@/api/writeOutcome'
import { reloadOutcomeMessage } from './reloadOutcomeText'

// Renders `key {param}={value}` so assertions can check both which key was
// chosen and what was interpolated into it.
const t = (key: string, params?: Record<string, string | number>) => {
  const rendered = Object.entries(params ?? {})
    .map(([name, value]) => `${name}=${value}`)
    .join(' ')
  return rendered ? `${key} ${rendered}` : key
}

function outcome(state: OutcomeState, extra: Partial<ReloadOutcome> = {}): ReloadOutcome {
  return { controllerId: 'east/controller-a', state, ...extra }
}

describe('reloadOutcomeMessage', () => {
  it('claims the reload finished only for converged', () => {
    const message = reloadOutcomeMessage(t, outcome('converged', {
      serverId: 'server-2',
      convergenceMs: 4200,
    }))

    expect(message.level).toBe('success')
    expect(message.text).toContain('center.reloadOutcome.converged')
    expect(message.text).toContain('serverId=server-2')
    expect(message.text).toContain('seconds=4.2')
  })

  it('reports accepted and unknown as unconfirmed, never as success', () => {
    for (const state of ['accepted', 'unknown'] as const) {
      const message = reloadOutcomeMessage(t, outcome(state))
      expect(message.level, `${state} must not read as success`).toBe('warning')
      expect(message.text).toContain(`center.reloadOutcome.${state === 'accepted' ? 'accepted' : 'unknown'}`)
    }
  })

  it('names the budget in the unknown wording so the operator knows what elapsed', () => {
    expect(reloadOutcomeMessage(t, outcome('unknown')).text).toContain('seconds=20')
  })

  it('surfaces the leader address for a follower refusal', () => {
    const message = reloadOutcomeMessage(t, outcome('conflict', { leader: '10.0.0.7:12101' }))

    expect(message.level).toBe('error')
    expect(message.text).toContain('center.reloadOutcome.conflict')
    expect(message.text).toContain('leader=10.0.0.7:12101')
  })

  it('still reports a follower refusal when the leader is unknown mid-election', () => {
    const message = reloadOutcomeMessage(t, outcome('conflict'))

    expect(message.level).toBe('error')
    expect(message.text).toContain('center.reloadOutcome.conflictNoLeader')
  })

  /// The pair the issue called out as collapsing into one generic toast: a
  /// transient "already in progress" is worth retrying, a plain rejection is
  /// not, and they must not share wording or severity.
  it('separates a transient retryable refusal from a plain rejection', () => {
    const transient = reloadOutcomeMessage(t, outcome('failed', {
      retryAfterSecs: 5,
      reason: 'the Controller refused the reload as temporarily unavailable: not ready',
    }))
    expect(transient.level).toBe('warning')
    expect(transient.text).toContain('center.reloadOutcome.retry')
    expect(transient.text).toContain('seconds=5')
    // The Controller refuses transiently for more than one reason; the message
    // must carry which, not paraphrase one of them.
    expect(transient.text).toContain('not ready')

    const rejected = reloadOutcomeMessage(t, outcome('failed', { reason: 'verb reload not granted' }))
    expect(rejected.level).toBe('error')
    expect(rejected.text).toContain('center.reloadOutcome.failed')
    expect(rejected.text).toContain('reason=verb reload not granted')
  })

  it('never reports success for any state other than converged', () => {
    const states: OutcomeState[] = ['superseded', 'accepted', 'conflict', 'failed', 'unknown']
    for (const state of states) {
      expect(reloadOutcomeMessage(t, outcome(state)).level, state).not.toBe('success')
    }
  })
})
