import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { WriteOutcome } from '@/api/writeOutcome'
import { WriteOutcomeList, WriteOutcomeTag } from './WriteOutcomeTag'

vi.mock('@/i18n', () => ({
  useT: () => (key: string, params?: Record<string, string | number>) => {
    if (!params) return key
    return `${key}:${Object.values(params).join(',')}`
  },
}))

function outcome(overrides: Partial<WriteOutcome>): WriteOutcome {
  return { controllerId: 'ctrl-a', state: 'converged', ...overrides }
}

describe('WriteOutcomeTag', () => {
  it('renders converged as a success-colored, distinct tag', () => {
    render(<WriteOutcomeTag outcome={outcome({ state: 'converged' })} />)
    const el = screen.getByTestId('write-outcome-converged')
    expect(el).toHaveTextContent('writeOutcome.state.converged')
  })

  it('renders superseded with the observed value surfaced, not as a failure', () => {
    render(
      <WriteOutcomeTag
        outcome={outcome({
          state: 'superseded',
          reason: 'overwritten after the write landed',
          observed: { spec: { data: { config: { active: 'strict' } } } },
        })}
      />,
    )
    const el = screen.getByTestId('write-outcome-superseded')
    expect(el).toHaveTextContent('writeOutcome.state.superseded')
    expect(el).toHaveTextContent('writeOutcome.detail.superseded')
    expect(el).toHaveTextContent('strict')
  })

  it('renders accepted distinctly from converged, noting local confirmation was not possible', () => {
    render(
      <WriteOutcomeTag
        outcome={outcome({
          state: 'accepted',
          reason: 'this replica does not hold the Controller session; convergence cannot be observed here',
        })}
      />,
    )
    const el = screen.getByTestId('write-outcome-accepted')
    expect(el).toHaveTextContent('writeOutcome.state.accepted')
    expect(el).toHaveTextContent('writeOutcome.detail.accepted')
    expect(screen.queryByTestId('write-outcome-converged')).toBeNull()
  })

  it('renders conflict telling the operator their view was stale', () => {
    render(<WriteOutcomeTag outcome={outcome({ state: 'conflict', reason: 'CAS precondition mismatch' })} />)
    const el = screen.getByTestId('write-outcome-conflict')
    expect(el).toHaveTextContent('writeOutcome.state.conflict')
    expect(el).toHaveTextContent('writeOutcome.detail.conflict')
  })

  it('renders failed and unknown as distinct states from each other and from success', () => {
    render(
      <>
        <WriteOutcomeTag outcome={outcome({ state: 'failed', reason: 'not in the local watch cache' })} />
        <WriteOutcomeTag outcome={outcome({ state: 'unknown', reason: 'no local convergence observed within 10s' })} />
      </>,
    )
    const failed = screen.getByTestId('write-outcome-failed')
    const unknown = screen.getByTestId('write-outcome-unknown')
    expect(failed).toHaveTextContent('writeOutcome.state.failed')
    expect(failed).toHaveTextContent('not in the local watch cache')
    expect(unknown).toHaveTextContent('writeOutcome.state.unknown')
    expect(unknown).toHaveTextContent('no local convergence observed within 10s')
    expect(failed.textContent).not.toBe(unknown.textContent)
  })

  it('assigns a distinct color to every one of the six states', () => {
    const states: WriteOutcome['state'][] = [
      'converged', 'superseded', 'accepted', 'conflict', 'failed', 'unknown',
    ]
    const { container } = render(
      <>
        {states.map((state) => <WriteOutcomeTag key={state} outcome={outcome({ state })} />)}
      </>,
    )
    const tags = states.map((state) =>
      container.querySelector(`[data-testid="write-outcome-${state}"] .ant-tag`))
    const colorClasses = tags.map((tag) => [...(tag?.classList ?? [])].find((c) => c.startsWith('ant-tag-')))
    expect(new Set(colorClasses).size).toBe(states.length)
  })
})

describe('WriteOutcomeList', () => {
  it('renders one labeled row per outcome and nothing when empty', () => {
    const { rerender } = render(<WriteOutcomeList items={[]} />)
    expect(screen.queryByTestId(/write-outcome-/)).toBeNull()

    rerender(
      <WriteOutcomeList
        items={[
          { key: 'a', label: 'ctrl-a', outcome: outcome({ state: 'converged' }) },
          { key: 'b', label: 'ctrl-b', outcome: outcome({ controllerId: 'ctrl-b', state: 'failed', reason: 'boom' }) },
        ]}
      />,
    )
    expect(screen.getByText('ctrl-a')).toBeInTheDocument()
    expect(screen.getByText('ctrl-b')).toBeInTheDocument()
    expect(screen.getByTestId('write-outcome-converged')).toBeInTheDocument()
    expect(screen.getByTestId('write-outcome-failed')).toHaveTextContent('boom')
  })
})
