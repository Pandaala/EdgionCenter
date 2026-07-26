import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import ControllersPage from './ControllersPage'

const mocks = vi.hoisted(() => ({
  listControllers: vi.fn(),
  listAdminControllers: vi.fn(),
  deleteAdminController: vi.fn(),
  reloadController: vi.fn(),
  useServerInfo: vi.fn(),
  useCan: vi.fn(),
}))

vi.mock('@/api/center', () => ({ centerApi: mocks }))
vi.mock('@/hooks/useServerInfo', () => ({ useServerInfo: mocks.useServerInfo }))
vi.mock('@/utils/permissions', () => ({ useCan: mocks.useCan }))
vi.mock('@/hooks/useControllerAccess', () => ({ invalidateControllerAccess: vi.fn() }))
vi.mock('@/i18n', () => ({ useT: () => (key: string, params?: Record<string, string | number>) => params?.n != null ? `${key}:${params.n}` : key }))

const controllers = [
  { controller_id: 'east/controller-a', cluster: 'east', env: ['prod'], tag: ['edge'], online: true, last_seen_secs_ago: 10, key_count: 4 },
  { controller_id: 'west/controller-b', cluster: 'west', env: [], tag: [], online: false, last_seen_secs_ago: null, key_count: null },
]

function renderPage() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<MemoryRouter><QueryClientProvider client={queryClient}><ControllersPage /></QueryClientProvider></MemoryRouter>)
}

describe('ControllersPage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.listControllers.mockResolvedValue({ success: true, count: controllers.length, data: controllers })
    mocks.listAdminControllers.mockResolvedValue({ success: true, count: 1, data: [{ controllerId: 'east/controller-a', cluster: 'east', env: ['prod'], tag: ['edge'], online: true, lastSeenAt: 1_700_000_000 }] })
    mocks.useCan.mockImplementation(
      (permission: string) => permission === 'controllers:read'
        || permission === 'controllers:write'
        || permission === 'proxy:access',
    )
  })

  it('does not request standalone history or expose mutations when unavailable', async () => {
    mocks.useServerInfo.mockReturnValue({ data: { data: { capabilities: { controllerHistory: false } } } })
    mocks.useCan.mockImplementation((permission: string) => permission === 'controllers:read')
    renderPage()
    expect(await screen.findByText('east/controller-a')).toBeInTheDocument()
    expect(mocks.listAdminControllers).not.toHaveBeenCalled()
    expect(screen.queryByText('center.admin.deleteController')).not.toBeInTheDocument()
    expect(screen.queryByText('center.reload')).not.toBeInTheDocument()
  })

  it('merges standalone history and exposes deletion for authorized users', async () => {
    mocks.useServerInfo.mockReturnValue({ data: { data: { capabilities: { controllerHistory: true } } } })
    renderPage()
    expect(await screen.findByText(new Date(1_700_000_000 * 1000).toLocaleString())).toBeInTheDocument()
    expect(screen.getAllByText('center.admin.deleteController')).toHaveLength(2)
    expect(mocks.listAdminControllers).toHaveBeenCalledTimes(1)
  })

  it('filters controllers by search text', async () => {
    mocks.useServerInfo.mockReturnValue({ data: { data: { capabilities: { controllerHistory: false } } } })
    renderPage()
    expect(await screen.findByText('west/controller-b')).toBeInTheDocument()
    fireEvent.change(screen.getByTestId('controller-search'), { target: { value: 'east' } })
    await waitFor(() => expect(screen.queryByText('west/controller-b')).not.toBeInTheDocument())
    expect(screen.getByText('east/controller-a')).toBeInTheDocument()
  })
})
