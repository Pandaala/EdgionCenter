import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Modal } from 'antd'
import LinkSysList from './LinkSysList'

const batchDelete = vi.fn()

vi.mock('@/api/resources', () => ({
  resourceApi: {
    delete: vi.fn(),
    batchDelete: (...args: unknown[]) => batchDelete(...args),
  },
}))

vi.mock('@/hooks/useResourceList', () => ({
  useResourceList: () => ({
    items: [
      { metadata: { namespace: 'default', name: 'redis-a' }, spec: { type: 'redis', config: { endpoints: [] } } },
      { metadata: { namespace: 'default', name: 'redis-b' }, spec: { type: 'redis', config: { endpoints: [] } } },
    ],
    isLoading: false,
    error: null,
    refetch: vi.fn(),
    fetchNextPage: vi.fn(),
    hasNextPage: false,
    isFetchingNextPage: false,
  }),
}))

vi.mock('@/components/ResourceEditor/LinkSys/LinkSysEditor', () => ({
  default: () => null,
}))

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return { ...actual, useParams: () => ({ controllerId: 'cluster/ctrl' }) }
})

function renderPage() {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <LinkSysList />
    </QueryClientProvider>,
  )
}

describe('LinkSysList', () => {
  beforeEach(() => {
    batchDelete.mockReset()
    batchDelete.mockResolvedValue(undefined)
    vi.spyOn(Modal, 'confirm').mockImplementation((config) => {
      void config.onOk?.()
      return { destroy: vi.fn(), update: vi.fn() }
    })
  })

  it('batch deletes the selected rows after confirmation', async () => {
    renderPage()
    await screen.findByText('redis-a')

    const checkboxes = screen.getAllByRole('checkbox')
    fireEvent.click(checkboxes[1])
    fireEvent.click(checkboxes[2])
    fireEvent.change(screen.getByPlaceholderText('Quick search...'), { target: { value: 'redis-a' } })
    fireEvent.click(await screen.findByRole('button', { name: 'Batch Delete' }))

    await waitFor(() => {
      expect(batchDelete).toHaveBeenCalledWith('linksys', [
        { namespace: 'default', name: 'redis-a' },
        { namespace: 'default', name: 'redis-b' },
      ])
    })
  })
})
