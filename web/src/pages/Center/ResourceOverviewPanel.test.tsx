import { render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { ControllerSummary } from '@/api/center'
import ResourceOverviewPanel from './ResourceOverviewPanel'

vi.mock('@/i18n', () => ({
  useT: () => (key: string, params?: Record<string, string | number>) => (
    params?.n == null ? key : `${key}:${params.n}`
  ),
}))

function controller(overrides: Partial<ControllerSummary>): ControllerSummary {
  return {
    controller_id: 'east/a',
    cluster: 'east',
    env: [],
    tag: [],
    online: true,
    key_count: null,
    per_kind: null,
    stats_state: 'missing',
    ...overrides,
  }
}

describe('ResourceOverviewPanel', () => {
  it('sums per_kind across online Controllers, keyed by catalog kind', () => {
    const controllers = [
      controller({
        controller_id: 'east/a',
        cluster: 'east',
        key_count: 3,
        per_kind: { HTTPRoute: 2, Gateway: 1 },
        stats_state: 'fresh',
      }),
      controller({
        controller_id: 'west/b',
        cluster: 'west',
        key_count: 2,
        per_kind: { HTTPRoute: 1, Service: 1 },
        stats_state: 'fresh',
      }),
    ]

    render(<ResourceOverviewPanel controllers={controllers} />)

    expect(within(screen.getByTestId('resource-count-total')).getByText('5')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-httproute')).getByText('3')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-gateway')).getByText('1')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-service')).getByText('1')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-grpcroute')).getByText('0')).toBeInTheDocument()
  })

  it('excludes offline Controllers from the sum even when they carry per_kind data', () => {
    const controllers = [
      controller({
        controller_id: 'east/a',
        online: true,
        per_kind: { HTTPRoute: 3 },
        stats_state: 'fresh',
      }),
      controller({
        controller_id: 'west/b',
        online: false,
        per_kind: { HTTPRoute: 9 },
        stats_state: 'stale',
      }),
    ]

    render(<ResourceOverviewPanel controllers={controllers} />)

    expect(within(screen.getByTestId('resource-count-total')).getByText('3')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-httproute')).getByText('3')).toBeInTheDocument()
  })

  it('marks the fleet total and every kind as a lower bound when an online Controller never reported stats', () => {
    const controllers = [
      controller({ controller_id: 'east/a', online: true, per_kind: null, stats_state: 'missing' }),
    ]

    render(<ResourceOverviewPanel controllers={controllers} />)

    expect(within(screen.getByTestId('resource-count-total')).getByText('0+')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-httproute')).getByText('—')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-gateway')).getByText('—')).toBeInTheDocument()
  })

  it('shows a "+" lower bound (not a dash) when the partial total is still greater than zero', () => {
    const controllers = [
      controller({
        controller_id: 'east/a',
        online: true,
        per_kind: { HTTPRoute: 4 },
        stats_state: 'fresh',
      }),
      controller({ controller_id: 'west/b', online: true, per_kind: null, stats_state: 'missing' }),
    ]

    render(<ResourceOverviewPanel controllers={controllers} />)

    expect(within(screen.getByTestId('resource-count-total')).getByText('4+')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-httproute')).getByText('4+')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-gateway')).getByText('—')).toBeInTheDocument()
  })

  it('treats a "fresh" Controller whose per_kind was dropped by the ingest bound as partial data, not a silent undercount', () => {
    // stats_state is derived from the total alone, so an oversized/malformed
    // per_kind map that got dropped at ingest still reports 'fresh' here
    // (the scalar total survived). Gating "reporting" on stats_state instead
    // of on per_kind presence would count this controller as fully reporting
    // while it contributes 0 to every kind, with no indicator of the gap.
    const controllers = [
      controller({
        controller_id: 'east/a',
        online: true,
        key_count: 200,
        per_kind: { HTTPRoute: 4 },
        stats_state: 'fresh',
      }),
      controller({
        controller_id: 'west/b',
        online: true,
        key_count: 500,
        per_kind: null,
        stats_state: 'fresh',
      }),
    ]

    render(<ResourceOverviewPanel controllers={controllers} />)

    // Must show the partial-data '+' convention, matching the "never
    // reported" case, rather than presenting 4 as the complete fleet total.
    expect(within(screen.getByTestId('resource-count-total')).getByText('4+')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-httproute')).getByText('4+')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-gateway')).getByText('—')).toBeInTheDocument()
  })

  it('shows an empty state when there are no online Controllers', () => {
    const controllers = [
      controller({ controller_id: 'east/a', online: false, stats_state: 'missing' }),
    ]

    render(<ResourceOverviewPanel controllers={controllers} />)

    expect(screen.getByText('center.resources.noOnlineControllers')).toBeInTheDocument()
    expect(screen.queryByTestId('resource-count-total')).not.toBeInTheDocument()
  })
})
