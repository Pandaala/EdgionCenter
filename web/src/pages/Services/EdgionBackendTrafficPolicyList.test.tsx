import { fireEvent, screen, waitFor } from '@testing-library/react'
import { Modal } from 'antd'
import type { ComponentProps } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { controllerMenu, type MenuNode } from '@/components/shell/menuConfig'
import { renderWithQueryClient } from '@/test/render'
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
    items: [{ apiVersion: 'edgion.io/v1alpha1', kind: 'EdgionBackendTrafficPolicy', metadata: { namespace: 'prod', name: 'policy-a', generation: 4 }, status: { ancestors: [{ ancestorRef: { name: 'provider' }, conditions: [{ type: 'Accepted', status: 'True', observedGeneration: 3 }] }] }, spec: { targetRefs: [{ group: 'edgion.io', kind: 'EdgionBackend', name: 'provider' }], healthCheck: { active: { type: 'https' } }, retryConstraint: {}, circuitBreaker: { maxParallelRequests: 20 }, connection: { connectTimeout: '3s' } } }],
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
    const collectPaths = (nodes: MenuNode[]): string[] => nodes.flatMap((node) => (
      node.kind === 'item' ? [node.path] : collectPaths(node.children)
    ))
    const paths = controllerMenu.flatMap((section) => collectPaths(section.children))
    expect(paths).toContain('/services/backend-traffic-policies')
  })

  it('shows probe protocol and every configured resilience section', () => {
    renderWithQueryClient(<EdgionBackendTrafficPolicyList />)
    expect(screen.queryByText('RoundRobin')).not.toBeInTheDocument()
    for (const label of ['Health Check · HTTPS', 'Retry Constraint', 'Circuit Breaker', 'Connection Override']) {
      expect(screen.getByText(label)).toBeVisible()
    }
  })

  it('marks an older attachment observation stale in the resource list', () => {
    renderWithQueryClient(<EdgionBackendTrafficPolicyList />)
    expect(screen.getByText('Accepted=True (stale)')).toHaveClass('ant-tag-gold')
  })

  it('keeps batch confirmation semantics when only one resource is selected', async () => {
    const confirm = vi.spyOn(Modal, 'confirm').mockReturnValue({ destroy: vi.fn(), update: vi.fn() })
    renderWithQueryClient(<EdgionBackendTrafficPolicyList />)

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
