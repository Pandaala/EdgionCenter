import { createElement, type ReactNode } from 'react'
import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { expect, it, vi } from 'vitest'
import { useResourceList } from './useResourceList'

const state = vi.hoisted(() => ({ controllerId: 'east/controller', list: vi.fn() }))
vi.mock('./useControllerMutationTarget', () => ({ useControllerMutationTarget: () => ({ controllerId: state.controllerId }) }))
vi.mock('@/api/resources', () => ({ resourceApi: { listAll: (...args: unknown[]) => state.list(...args) }, clusterResourceApi: {} }))

it('isolates source pages on Controller switches even when callers omit scope', async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  state.list.mockImplementation(async (_kind, options) => ({ success: true, count: 1,
    data: [{ kind: 'HTTPRoute', apiVersion: 'v1', metadata: { name: options.target.controllerId } }],
  }))
  const wrapper = ({ children }: { children: ReactNode }) => createElement(QueryClientProvider, { client }, children)
  const view = renderHook(() => useResourceList('httproute', { namespaced: true }), { wrapper })
  await waitFor(() => expect(view.result.current.items[0]?.metadata.name).toBe('east/controller'))
  state.controllerId = 'west/controller'
  view.rerender()
  expect(view.result.current.items).toEqual([])
  await waitFor(() => expect(view.result.current.items[0]?.metadata.name).toBe('west/controller'))
  expect(state.list.mock.calls[0][1].target.controllerId).toBe('east/controller')
  expect(state.list.mock.calls[1][1].target.controllerId).toBe('west/controller')
  view.unmount()
  client.clear()
})
