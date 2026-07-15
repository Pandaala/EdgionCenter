import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import LinkSysEditor from './LinkSysEditor'

const create = vi.fn()

vi.mock('@/api/resources', () => ({
  resourceApi: {
    create: (...args: unknown[]) => create(...args),
    update: vi.fn(),
  },
}))

vi.mock('./LinkSysForm', () => ({
  default: ({ data, onChange }: any) => (
    <button
      onClick={() => onChange({
        ...data,
        metadata: { name: 'redis-test', namespace: 'default' },
        spec: {
          type: 'redis',
          config: {
            endpoints: ['redis://127.0.0.1:6379'],
            topology: { mode: 'standalone' },
          },
        },
      })}
    >
      Fill valid LinkSys
    </button>
  ),
}))

vi.mock('@/components/YamlEditor', () => ({
  default: () => null,
}))

describe('LinkSysEditor', () => {
  beforeEach(() => {
    create.mockReset()
    create.mockResolvedValue({ success: true })
  })

  it('creates the current wire shape and invalidates the resource list', async () => {
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
    const invalidate = vi.spyOn(client, 'invalidateQueries')

    render(
      <QueryClientProvider client={client}>
        <LinkSysEditor visible mode="create" onClose={vi.fn()} />
      </QueryClientProvider>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Fill valid LinkSys' }))
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))

    await waitFor(() => expect(create).toHaveBeenCalledOnce())
    expect(create.mock.calls[0][0]).toBe('linksys')
    expect(create.mock.calls[0][1]).toBe('default')
    expect(create.mock.calls[0][2]).toContain('config:')
    expect(create.mock.calls[0][2]).toContain('endpoints:')
    expect(create.mock.calls[0][2]).not.toContain('password:')
    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ['resource-list', 'linksys'] })
    })
  })
})
