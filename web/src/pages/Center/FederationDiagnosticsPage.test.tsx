import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import FederationDiagnosticsPage from './FederationDiagnosticsPage'

const mocks = vi.hoisted(() => ({ watchStatus: vi.fn(), metadataStoreStatus: vi.fn() }))
vi.mock('@/api/center', () => ({ centerApi: mocks }))

describe('FederationDiagnosticsPage', () => {
  it('renders watch ownership and effective metadata coverage', async () => {
    mocks.watchStatus.mockResolvedValue({ success: true, count: 2, data: [
      { controllerId: 'east/controller-a', syncVersion: 7, serverId: 'center-a' },
      { controllerId: 'west/controller-b', syncVersion: 8, serverId: 'center-b' },
    ] })
    mocks.metadataStoreStatus.mockResolvedValue({ success: true, data: {
      regionRoutes: [{ key: 'shop/region/primary', controllerCount: 2 }],
      globalConnectionIpRestrictions: [{ key: 'shop/global-ip', controllerCount: 2 }],
    } })
    render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><FederationDiagnosticsPage /></QueryClientProvider>)
    expect(await screen.findByText('east/controller-a')).toBeInTheDocument()
    expect(screen.getByText('west/controller-b')).toBeInTheDocument()
    expect(await screen.findByText('shop/region/primary')).toBeInTheDocument()
    expect(screen.getByText('shop/global-ip')).toBeInTheDocument()
  })
})
