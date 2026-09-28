import { createElement, type ReactNode } from 'react'
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { message } from 'antd'
import { useResourceList } from './useResourceList'

const state = vi.hoisted(() => ({ controllerId: 'east/controller', list: vi.fn() }))
vi.mock('./useControllerMutationTarget', () => ({ useControllerMutationTarget: () => ({ controllerId: state.controllerId }) }))
vi.mock('@/api/resources', () => ({ resourceApi: { listAll: (...args: unknown[]) => state.list(...args) }, clusterResourceApi: { listAll: (...args: unknown[]) => state.list(...args) } }))
const page = (name: string, token?: string) => ({ success: true, data: [{ apiVersion: 'v1', kind: 'Service', metadata: { name } }], continue_token: token })
const stale = { response: { status: 410, data: { code: 'StalePagination' } } }
beforeEach(() => { state.controllerId = 'east/controller'; state.list.mockReset(); vi.spyOn(message, 'info').mockImplementation(() => undefined as any) })
afterEach(() => { cleanup(); vi.restoreAllMocks() })
function setup(namespaced = true) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => createElement(QueryClientProvider, { client }, children)
  const view = renderHook(() => useResourceList(namespaced ? 'service' : 'gatewayclass', { namespaced }), { wrapper })
  return { ...view, client }
}

it.each([true, false])('restarts from page one after a stale continuation (namespaced=%s)', async (namespaced) => {
  state.list.mockResolvedValueOnce(page('old', 'expired')).mockRejectedValueOnce(stale).mockResolvedValue(page('fresh'))
  const view = setup(namespaced)
  await waitFor(() => expect(view.result.current.hasNextPage).toBe(true))
  await act(async () => { await view.result.current.fetchNextPage() })
  await waitFor(() => expect(view.result.current.items.map((r) => r.metadata.name)).toEqual(['fresh']))
  expect(state.list.mock.calls.map(([, options]) => options.continue)).toEqual([undefined, 'expired', undefined])
  expect(view.result.current.error).toBeNull()
  expect(message.info).toHaveBeenCalledTimes(1)
  view.unmount(); view.client.clear()
})


it('leaves a persistent stale error visible instead of looping and allows manual recovery', async () => {
  state.list.mockResolvedValueOnce(page('old', 'expired')).mockRejectedValue(stale)
  const view = setup()
  await waitFor(() => expect(view.result.current.hasNextPage).toBe(true))
  await act(async () => { await view.result.current.fetchNextPage() })
  await waitFor(() => expect(state.list).toHaveBeenCalledTimes(3))
  await waitFor(() => expect(view.result.current.error).toEqual(stale))
  expect(view.result.current.items).toEqual([])
  expect(message.info).toHaveBeenCalledTimes(1)
  const later = Date.now() + 10_000
  vi.spyOn(Date, 'now').mockReturnValue(later)
  await act(async () => { await view.result.current.refetch() })
  await waitFor(() => expect(view.result.current.error).toEqual(stale))
  expect(state.list).toHaveBeenCalledTimes(4)
  expect(message.info).toHaveBeenCalledTimes(1)
  state.list.mockResolvedValue(page('recovered'))
  await act(async () => { await view.result.current.refetch() })
  await waitFor(() => expect(view.result.current.items[0]?.metadata.name).toBe('recovered'))
  view.unmount(); view.client.clear()
})

it('recovers a different Controller within the dedup window without resetting unrelated queries', async () => {
  state.list.mockImplementation(async (_kind, options) => {
    if (options.continue) throw stale
    return page(options.target.controllerId, 'expired')
  })
  const view = setup()
  const unrelated = ['resource-list', 'service', null, 50, 'unrelated', 'unrelated']
  view.client.setQueryData(unrelated, { marker: 'keep' })
  await waitFor(() => expect(view.result.current.hasNextPage).toBe(true))
  await act(async () => { await view.result.current.fetchNextPage() })
  await waitFor(() => expect(state.list).toHaveBeenCalledTimes(3))
  state.controllerId = 'west/controller'
  view.rerender()
  await waitFor(() => expect(view.result.current.items[0]?.metadata.name).toBe('west/controller'))
  await act(async () => { await view.result.current.fetchNextPage() })
  await waitFor(() => expect(state.list).toHaveBeenCalledTimes(6))
  await waitFor(() => expect(view.result.current.error).toBeNull())
  expect(view.result.current.items[0]?.metadata.name).toBe('west/controller')
  expect(view.client.getQueryData(unrelated)).toEqual({ marker: 'keep' })
  expect(message.info).toHaveBeenCalledTimes(2)
  view.unmount(); view.client.clear()
})
