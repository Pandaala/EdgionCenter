import { render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import ResourceOverviewPanel from './ResourceOverviewPanel'

const mocks = vi.hoisted(() => ({
  useControllerResourceSnapshots: vi.fn(),
}))

vi.mock('@/hooks/useControllerResourceSnapshots', () => ({
  useControllerResourceSnapshots: mocks.useControllerResourceSnapshots,
}))
vi.mock('@/i18n', () => ({
  useT: () => (key: string, params?: Record<string, string | number>) => (
    params?.n == null ? key : `${key}:${params.n}`
  ),
}))

const controllers = [
  {
    controller_id: 'east/a',
    cluster: 'east',
    env: [],
    tag: [],
    online: true,
    key_count: 3,
  },
  {
    controller_id: 'west/b',
    cluster: 'west',
    env: [],
    tag: [],
    online: true,
    key_count: 2,
  },
]

describe('ResourceOverviewPanel', () => {
  it('shows the fleet total and a count for every resource kind', () => {
    mocks.useControllerResourceSnapshots.mockReturnValue({
      snapshots: [
        {
          controllerId: 'east/a',
          cluster: 'east',
          resources: {
            httproute: [{ metadata: { name: 'a' } }, { metadata: { name: 'b' } }],
            gateway: [{ metadata: { name: 'edge' } }],
          },
          errors: [],
        },
        {
          controllerId: 'west/b',
          cluster: 'west',
          resources: {
            httproute: [{ metadata: { name: 'c' } }],
            service: [{ metadata: { name: 'api' } }],
          },
          errors: [],
        },
      ],
      isLoading: false,
      isFetching: false,
    })

    render(<ResourceOverviewPanel controllers={controllers} />)

    expect(within(screen.getByTestId('resource-count-total')).getByText('5')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-httproute')).getByText('3')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-gateway')).getByText('1')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-service')).getByText('1')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-grpcroute')).getByText('0')).toBeInTheDocument()
  })

  it('uses lower-bound and unavailable values without showing a global warning', () => {
    mocks.useControllerResourceSnapshots.mockReturnValue({
      snapshots: [{
        controllerId: 'east/a',
        cluster: 'east',
        resources: {},
        errors: ['httproute', 'gateway'],
      }],
      isLoading: false,
      isFetching: false,
    })

    render(<ResourceOverviewPanel controllers={controllers.slice(0, 1)} />)

    expect(within(screen.getByTestId('resource-count-total')).getByText('0+')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-httproute')).getByText('—')).toBeInTheDocument()
    expect(within(screen.getByTestId('resource-count-gateway')).getByText('—')).toBeInTheDocument()
    expect(screen.queryByText('center.resources.partial')).not.toBeInTheDocument()
  })
})
