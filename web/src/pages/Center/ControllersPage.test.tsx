import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { Modal } from 'antd'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { renderWithQueryClient } from '@/test/render'
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
  { controller_id: 'east/controller-a', cluster: 'east', env: ['prod'], tag: ['edge'], online: true, last_seen_secs_ago: 10, key_count: 4, stats_state: 'fresh' },
  { controller_id: 'west/controller-b', cluster: 'west', env: [], tag: [], online: false, last_seen_secs_ago: null, key_count: null, stats_state: 'missing' },
]

function renderPage() {
  return renderWithQueryClient(<MemoryRouter><ControllersPage /></MemoryRouter>)
}

describe('ControllersPage', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.listControllers.mockResolvedValue({ success: true, count: controllers.length, data: controllers })
    mocks.listAdminControllers.mockResolvedValue({ success: true, count: 1, data: [{ controllerId: 'east/controller-a', cluster: 'east', env: ['prod'], tag: ['edge'], online: true, lastSeenAt: 1_700_000_000 }] })
    mocks.useCan.mockImplementation(
      (permission: string) => permission === 'controllers:read'
        || permission === 'controllers:write'
        || permission === 'proxy:read',
    )
  })

  // Imperative antd modals render outside the RTL container, so its automatic
  // cleanup does not remove them and they would leak into the next test.
  afterEach(() => {
    Modal.destroyAll()
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

  it('renders the Resources column per stats_state: missing, fresh, and stale', async () => {
    mocks.useServerInfo.mockReturnValue({ data: { data: { capabilities: { controllerHistory: false } } } })
    mocks.listControllers.mockResolvedValue({
      success: true,
      count: 3,
      data: [
        { controller_id: 'a/missing', cluster: 'a', env: [], tag: [], online: true, last_seen_secs_ago: 1, key_count: null, stats_state: 'missing' },
        { controller_id: 'b/fresh', cluster: 'b', env: [], tag: [], online: true, last_seen_secs_ago: 1, key_count: 12, stats_state: 'fresh' },
        { controller_id: 'c/stale', cluster: 'c', env: [], tag: [], online: false, last_seen_secs_ago: 900, key_count: 7, stats_state: 'stale' },
      ],
    })
    renderPage()

    expect(await screen.findByText('b/fresh')).toBeInTheDocument()
    const rows = screen.getAllByRole('row').filter((row) => row.querySelector('td'))
    const missingRow = rows.find((row) => row.textContent?.includes('a/missing'))!
    const freshRow = rows.find((row) => row.textContent?.includes('b/fresh'))!
    const staleRow = rows.find((row) => row.textContent?.includes('c/stale'))!

    expect(within(missingRow).getByText('—')).toBeInTheDocument()
    expect(within(freshRow).getByText('12')).toBeInTheDocument()
    expect(within(staleRow).getByText('7')).toBeInTheDocument()
    expect(within(staleRow).getByText('globalResources.syncState.stale')).toBeInTheDocument()
  })

  /// Confirms a reload and returns once the mutation has been dispatched with
  /// the expected controller id. React Query v5 hands `mutationFn` a second
  /// (context) argument, so the first argument is asserted rather than the
  /// whole call signature.
  async function confirmReload(controllerId: string) {
    fireEvent.click(screen.getAllByTestId('controller-reload')[0])
    fireEvent.click(await screen.findByTestId('controller-reload-confirm'))
    await waitFor(() => expect(mocks.reloadController).toHaveBeenCalled())
    expect(mocks.reloadController.mock.calls[0][0]).toBe(controllerId)
  }

  /// The regression this page had: a reload that was only dispatched rendered
  /// as an unqualified green toast. Any state other than `converged` must now
  /// raise a modal the operator has to dismiss.
  it('raises a modal for a reload that was dispatched but not confirmed', async () => {
    mocks.useServerInfo.mockReturnValue({ data: { data: { capabilities: { controllerHistory: false } } } })
    mocks.reloadController.mockResolvedValue({
      controllerId: 'east/controller-a',
      state: 'unknown',
      reason: 'reload was dispatched, but no new server_id was observed within 20s',
    })
    renderPage()

    expect(await screen.findByText('east/controller-a')).toBeInTheDocument()
    await confirmReload('east/controller-a')

    expect(await screen.findByTestId('controller-reload-outcome-ok')).toBeInTheDocument()
    // antd renders a confirm title in two nodes, so match all of them.
    expect((await screen.findAllByText('center.reloadOutcome.title')).length).toBeGreaterThan(0)
  })

  it('does not raise a modal when the reload actually completed', async () => {
    mocks.useServerInfo.mockReturnValue({ data: { data: { capabilities: { controllerHistory: false } } } })
    mocks.reloadController.mockResolvedValue({
      controllerId: 'east/controller-a',
      state: 'converged',
      serverId: 'server-2',
      convergenceMs: 4200,
    })
    renderPage()

    expect(await screen.findByText('east/controller-a')).toBeInTheDocument()
    await confirmReload('east/controller-a')

    // The confirm dialog closes on its own; nothing else may appear. Waiting
    // for its OK button to go away is what makes this assertion meaningful
    // rather than merely early.
    await waitFor(() => expect(screen.queryByTestId('controller-reload-confirm')).not.toBeInTheDocument())
    expect(screen.queryByTestId('controller-reload-outcome-ok')).not.toBeInTheDocument()
    expect(screen.queryAllByText('center.reloadOutcome.title')).toHaveLength(0)
  })
})
