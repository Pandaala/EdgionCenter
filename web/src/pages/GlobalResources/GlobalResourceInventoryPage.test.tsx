import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { globalResourcesApi } from '@/api/globalResources'
import GlobalResourceInventoryPage from './GlobalResourceInventoryPage'
import { GLOBAL_RESOURCE_DESCRIPTORS } from './globalResourceDescriptors'

vi.mock('@/i18n', () => ({
  useT: () => (key: string) => key,
}))

vi.mock('@/components/YamlEditor', () => ({
  default: ({ value, readOnly }: { value: string; readOnly: boolean }) => (
    <pre data-testid="yaml-editor" data-readonly={String(readOnly)}>{value}</pre>
  ),
}))

vi.mock('@/api/globalResources', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/api/globalResources')>()
  return {
    ...original,
    globalResourcesApi: {
      catalog: vi.fn(),
      list: vi.fn(),
      detail: vi.fn(),
      preflight: vi.fn(),
    },
  }
})

const catalog = {
  catalogRevision: 'catalog-1',
  configRevision: 'config-1',
  platformNamespaces: ['edgion-system'],
  kinds: [],
  clusters: [
    { cluster: 'alpha', state: 'available' as const, controllerId: 'controller-a', candidates: [] },
    { cluster: 'beta', state: 'offline' as const, controllerId: null, candidates: [] },
  ],
}

const inventory = {
  catalogRevision: 'catalog-1',
  configRevision: 'config-1',
  membershipRevision: 'membership-1',
  inventoryRevision: 'inventory-1',
  kind: 'EdgionConfigData' as const,
  configDataType: 'IpList' as const,
  clusters: [
    {
      ...catalog.clusters[0],
      complete: false,
      errors: [{
        namespace: 'edgion-data',
        code: 'upstream_unavailable' as const,
        status: 503,
        retryable: true,
      }],
    },
    { ...catalog.clusters[1], complete: false, errors: [] },
  ],
  groups: [{
    key: { kind: 'EdgionConfigData', namespace: 'edgion-system', name: 'trusted-proxies' },
    members: [{
      cluster: 'alpha',
      controllerId: 'controller-a',
      object: {
        apiVersion: 'edgion.io/v1',
        kind: 'EdgionConfigData',
        metadata: { namespace: 'edgion-system', name: 'trusted-proxies' },
        data: { type: 'IpList', values: ['192.0.2.1'] },
      },
    }],
  }],
  continueToken: null,
}

function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  })
  return render(
    <QueryClientProvider client={client}>
      <GlobalResourceInventoryPage descriptor={GLOBAL_RESOURCE_DESCRIPTORS[0]} />
    </QueryClientProvider>,
  )
}

describe('GlobalResourceInventoryPage', () => {
  beforeEach(() => {
    vi.mocked(globalResourcesApi.catalog).mockReset().mockResolvedValue(catalog)
    vi.mocked(globalResourcesApi.list).mockReset().mockResolvedValue(inventory)
    vi.mocked(globalResourcesApi.detail).mockReset().mockResolvedValue({
      ...inventory.clusters[0],
      kind: 'EdgionConfigData',
      namespace: 'edgion-system',
      name: 'trusted-proxies',
      object: inventory.groups[0].members[0].object,
    })
  })

  it('renders inventory rows without the cluster coverage card', async () => {
    mount()

    expect(screen.getByRole('heading', { name: 'IpList' })).toBeInTheDocument()
    expect(screen.queryByText('EdgionConfigData · IpList')).not.toBeInTheDocument()
    expect(await screen.findByText('trusted-proxies')).toBeInTheDocument()
    expect(screen.queryByText('globalResources.clusterSummary.title')).not.toBeInTheDocument()
    expect(screen.queryByTestId('global-resource-cluster-error')).not.toBeInTheDocument()
  })

  it('does not request detail until a concrete member is selected', async () => {
    mount()
    fireEvent.click(await screen.findByText('globalResources.action.compare'))

    expect(globalResourcesApi.detail).not.toHaveBeenCalled()
    fireEvent.click(screen.getByTestId('global-resource-member-alpha'))

    await waitFor(() => {
      expect(globalResourcesApi.detail).toHaveBeenCalledWith(
        'edgion-config-data',
        'edgion-system',
        'trusted-proxies',
        'alpha',
      )
    })
    expect(await screen.findByTestId('yaml-editor')).toHaveAttribute('data-readonly', 'true')
  })

  it('shows structured fresh-read errors and falls back to the inventory snapshot', async () => {
    vi.mocked(globalResourcesApi.detail).mockResolvedValue({
      ...inventory.clusters[0],
      complete: false,
      errors: [{
        namespace: 'edgion-system',
        code: 'upstream_rejected',
        status: 403,
        retryable: false,
      }],
      kind: 'EdgionConfigData',
      namespace: 'edgion-system',
      name: 'trusted-proxies',
      object: {
        apiVersion: 'edgion.io/v1',
        kind: 'EdgionConfigData',
        metadata: { namespace: 'edgion-system', name: 'must-not-be-shown' },
        data: { type: 'IpList', values: [] },
      },
    })
    mount()
    fireEvent.click(await screen.findByText('globalResources.action.compare'))
    fireEvent.click(screen.getByTestId('global-resource-member-alpha'))

    expect(await screen.findByText('globalResources.drawer.detailFailed')).toBeInTheDocument()
    expect(screen.getByText('globalResources.drawer.showingSnapshot')).toBeInTheDocument()
    expect(screen.getByTestId('global-resource-detail-errors')).toHaveTextContent(
      'edgion-system · globalResources.errorCode.upstream_rejected',
    )
    expect(screen.getByTestId('yaml-editor')).toHaveTextContent('name: trusted-proxies')
    expect(screen.getByTestId('yaml-editor')).not.toHaveTextContent('must-not-be-shown')
  })

  it('reports a missing fresh object and falls back to the inventory snapshot', async () => {
    vi.mocked(globalResourcesApi.detail).mockResolvedValue({
      ...inventory.clusters[0],
      complete: true,
      errors: [],
      kind: 'EdgionConfigData',
      namespace: 'edgion-system',
      name: 'trusted-proxies',
      object: null,
    })
    mount()
    fireEvent.click(await screen.findByText('globalResources.action.compare'))
    fireEvent.click(screen.getByTestId('global-resource-member-alpha'))

    expect(await screen.findByText('globalResources.drawer.freshObjectMissing')).toBeInTheDocument()
    expect(screen.getByText('globalResources.drawer.showingSnapshot')).toBeInTheDocument()
    expect(screen.getByTestId('yaml-editor')).toHaveTextContent('name: trusted-proxies')
  })

  it('reports an incomplete fresh read even when it has no structured errors', async () => {
    vi.mocked(globalResourcesApi.detail).mockResolvedValue({
      ...inventory.clusters[0],
      complete: false,
      errors: [],
      kind: 'EdgionConfigData',
      namespace: 'edgion-system',
      name: 'trusted-proxies',
      object: inventory.groups[0].members[0].object,
    })
    mount()
    fireEvent.click(await screen.findByText('globalResources.action.compare'))
    fireEvent.click(screen.getByTestId('global-resource-member-alpha'))

    expect(await screen.findByText('globalResources.drawer.freshReadIncomplete')).toBeInTheDocument()
    expect(screen.getByText('globalResources.drawer.showingSnapshot')).toBeInTheDocument()
  })

  it('uses the descriptor type without rendering a duplicate type switcher', async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    })
    render(
      <QueryClientProvider client={client}>
        <GlobalResourceInventoryPage descriptor={GLOBAL_RESOURCE_DESCRIPTORS[0]} />
      </QueryClientProvider>,
    )

    await waitFor(() => {
      expect(globalResourcesApi.list).toHaveBeenCalledWith(
        'edgion-config-data',
        expect.objectContaining({ configDataType: 'IpList' }),
      )
    })
    expect(screen.queryByText('RegionRouteOverride')).not.toBeInTheDocument()
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument()
  })
})
