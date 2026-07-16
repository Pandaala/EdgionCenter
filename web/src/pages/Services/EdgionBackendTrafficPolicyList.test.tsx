import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Modal } from 'antd'
import type { ComponentProps } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { controllerMenu } from '@/components/shell/menuConfig'
import EdgionBackendTrafficPolicyList from './EdgionBackendTrafficPolicyList'

vi.mock('@/hooks/useControllerMutationTarget', () => ({ useControllerMutationTarget: () => ({ controllerId: 'cluster/controller' }) }))
vi.mock('@/components/resource/PermissionAwareButton', () => ({
  default: (props: ComponentProps<'button'> & { danger?: boolean; resourceKind?: string; resourceVerb?: string }) => {
    const { danger, resourceKind, resourceVerb, ...buttonProps } = props
    void danger
    void resourceKind
    void resourceVerb
    return <button {...buttonProps} />
  },
}))
vi.mock('@/hooks/useResourceList', () => ({
  useResourceList: () => ({
    items: [{ apiVersion: 'edgion.io/v1alpha1', kind: 'EdgionBackendTrafficPolicy', metadata: { namespace: 'prod', name: 'policy-a' }, spec: { targetRefs: [] } }],
    isLoading: false,
    error: null,
    refetch: vi.fn(),
    fetchNextPage: vi.fn(),
    hasNextPage: false,
    isFetchingNextPage: false,
  }),
}))
vi.mock('@/components/ResourceEditor/EdgionBackendTrafficPolicy/EdgionBackendTrafficPolicyEditor', () => ({ default: () => null }))
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return { ...actual, useParams: () => ({ controllerId: 'cluster/controller' }) }
})

describe('EdgionBackendTrafficPolicy navigation', () => {
  beforeEach(() => { vi.restoreAllMocks() })

  it('registers the canonical services route in the controller menu', () => {
    const paths = controllerMenu.flatMap((section) => section.children.flatMap((entry) => (
      entry.kind === 'group' ? entry.children.map((item) => item.path) : [entry.path]
    )))
    expect(paths).toContain('/services/backend-traffic-policies')
  })

  it('keeps batch confirmation semantics when only one resource is selected', async () => {
    const confirm = vi.spyOn(Modal, 'confirm').mockReturnValue({ destroy: vi.fn(), update: vi.fn() })
    render(
      <QueryClientProvider client={new QueryClient()}>
        <EdgionBackendTrafficPolicyList />
      </QueryClientProvider>,
    )

    fireEvent.click(screen.getAllByRole('checkbox').at(-1)!)
    const batchDelete = await screen.findByTestId('edgionbackendtrafficpolicy-batch-delete')
    await waitFor(() => expect(batchDelete).toBeEnabled())
    fireEvent.click(batchDelete)

    expect(confirm).toHaveBeenCalledWith(expect.objectContaining({
      title: 'Batch Delete',
      cancelButtonProps: { 'data-testid': 'resource-batch-delete-cancel' },
    }))
  })
})
