import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import CenterDashboard from './CenterDashboard'

const mocks = vi.hoisted(() => ({
  listControllers: vi.fn(),
  useCan: vi.fn(),
}))

vi.mock('@/api/center', () => ({ centerApi: { listControllers: mocks.listControllers } }))
vi.mock('@/utils/permissions', () => ({ useCan: mocks.useCan }))
vi.mock('@/i18n', () => ({ useT: () => (key: string) => key }))
vi.mock('./ResourceOverviewPanel', () => ({
  default: () => <div data-testid="resource-overview" />,
}))

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(
    <QueryClientProvider client={queryClient}>
      <CenterDashboard />
    </QueryClientProvider>,
  )
}

describe('CenterDashboard', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.listControllers.mockResolvedValue({ success: true, count: 0, data: [] })
  })

  it('shows the fleet overview for a Controller reader', async () => {
    mocks.useCan.mockImplementation((permission: string) => permission === 'controllers:read')
    mocks.listControllers.mockResolvedValue({
      success: true,
      count: 2,
      data: [
        { controller_id: 'east/a', cluster: 'east', env: [], tag: [], online: true, key_count: 4 },
        { controller_id: 'west/b', cluster: 'west', env: [], tag: [], online: false, key_count: 6 },
      ],
    })
    renderPage()

    expect(await screen.findByTestId('resource-overview')).toBeInTheDocument()
    expect(screen.getByText('center.dashboard.controllers')).toBeInTheDocument()
    expect(screen.getByText('center.dashboard.online')).toBeInTheDocument()
    expect(screen.getByText('center.dashboard.clusters')).toBeInTheDocument()
  })

  it('renders a bounded empty state without Controller read access', () => {
    mocks.useCan.mockReturnValue(false)
    renderPage()

    expect(screen.getByText('center.dashboard.noAccess')).toBeInTheDocument()
    expect(mocks.listControllers).not.toHaveBeenCalled()
  })
})
