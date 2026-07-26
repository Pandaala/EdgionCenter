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
  kind: 'HTTPRoute' as const,
  configDataType: null,
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
    key: { kind: 'HTTPRoute', namespace: 'edgion-system', name: 'public-route' },
    members: [{
      cluster: 'alpha',
      controllerId: 'controller-a',
      object: {
        apiVersion: 'gateway.networking.k8s.io/v1',
        kind: 'HTTPRoute',
        metadata: { namespace: 'edgion-system', name: 'public-route' },
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
      kind: 'HTTPRoute',
      namespace: 'edgion-system',
      name: 'public-route',
      object: inventory.groups[0].members[0].object,
    })
  })

  it('renders incomplete and unavailable clusters without treating them as empty', async () => {
    mount()

    expect(await screen.findByText('public-route')).toBeInTheDocument()
    expect(screen.getAllByText(/beta: globalResources.clusterState.offline/)).toHaveLength(2)
    expect(screen.getAllByText(/globalResources.clusterState.incomplete/).length).toBeGreaterThan(0)
    expect(screen.getByTestId('global-resource-cluster-error')).toHaveTextContent(
      'edgion-data · globalResources.errorCode.upstream_unavailable',
    )
  })

  it('does not request detail until a concrete member is selected', async () => {
    mount()
    fireEvent.click(await screen.findByText('globalResources.action.compare'))

    expect(globalResourcesApi.detail).not.toHaveBeenCalled()
    fireEvent.click(screen.getByTestId('global-resource-member-alpha'))

    await waitFor(() => {
      expect(globalResourcesApi.detail).toHaveBeenCalledWith(
        'http-route',
        'edgion-system',
        'public-route',
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
      kind: 'HTTPRoute',
      namespace: 'edgion-system',
      name: 'public-route',
      object: {
        apiVersion: 'gateway.networking.k8s.io/v1',
        kind: 'HTTPRoute',
        metadata: { namespace: 'edgion-system', name: 'must-not-be-shown' },
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
    expect(screen.getByTestId('yaml-editor')).toHaveTextContent('name: public-route')
    expect(screen.getByTestId('yaml-editor')).not.toHaveTextContent('must-not-be-shown')
  })

  it('reports a missing fresh object and falls back to the inventory snapshot', async () => {
    vi.mocked(globalResourcesApi.detail).mockResolvedValue({
      ...inventory.clusters[0],
      complete: true,
      errors: [],
      kind: 'HTTPRoute',
      namespace: 'edgion-system',
      name: 'public-route',
      object: null,
    })
    mount()
    fireEvent.click(await screen.findByText('globalResources.action.compare'))
    fireEvent.click(screen.getByTestId('global-resource-member-alpha'))

    expect(await screen.findByText('globalResources.drawer.freshObjectMissing')).toBeInTheDocument()
    expect(screen.getByText('globalResources.drawer.showingSnapshot')).toBeInTheDocument()
    expect(screen.getByTestId('yaml-editor')).toHaveTextContent('name: public-route')
  })

  it('reports an incomplete fresh read even when it has no structured errors', async () => {
    vi.mocked(globalResourcesApi.detail).mockResolvedValue({
      ...inventory.clusters[0],
      complete: false,
      errors: [],
      kind: 'HTTPRoute',
      namespace: 'edgion-system',
      name: 'public-route',
      object: inventory.groups[0].members[0].object,
    })
    mount()
    fireEvent.click(await screen.findByText('globalResources.action.compare'))
    fireEvent.click(screen.getByTestId('global-resource-member-alpha'))

    expect(await screen.findByText('globalResources.drawer.freshReadIncomplete')).toBeInTheDocument()
    expect(screen.getByText('globalResources.drawer.showingSnapshot')).toBeInTheDocument()
  })

  it('uses one EdgionConfigData entry point before a type is selected', async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    })
    render(
      <QueryClientProvider client={client}>
        <GlobalResourceInventoryPage descriptor={GLOBAL_RESOURCE_DESCRIPTORS[3]} />
      </QueryClientProvider>,
    )

    await waitFor(() => {
      expect(globalResourcesApi.list).toHaveBeenCalledWith(
        'edgion-config-data',
        expect.objectContaining({ configDataType: undefined }),
      )
    })

    fireEvent.click(screen.getByText('IpList'))
    await waitFor(() => {
      expect(globalResourcesApi.list).toHaveBeenCalledWith(
        'edgion-config-data',
        expect.objectContaining({ configDataType: 'IpList' }),
      )
    })
  })
})
