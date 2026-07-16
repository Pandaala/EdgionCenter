import { createElement, type ReactNode } from 'react'
import { act, renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  CONTROLLER_ACCESS_RESOURCE_KINDS,
  CONTROLLER_KIND_BY_RESOURCE_KIND,
  controllerAccessApi,
} from '@/api/access'
import { CONTROLLER_ACCESS_OPERATIONS, type ControllerAccessDocument } from '@/api/types'
import { getResourceCatalogEntry } from '@/config/resourceCatalog'
import { controllerAccessQueryKey, normalizeControllerAccessId, useControllerAccess } from './useControllerAccess'

function accessDocument(updateAllowed: boolean): ControllerAccessDocument {
  return {
    schemaVersion: 1,
    revision: `sha256:${(updateAllowed ? 'a' : 'b').repeat(64)}`,
    resources: CONTROLLER_ACCESS_RESOURCE_KINDS.map((kind) => ({
      kind: CONTROLLER_KIND_BY_RESOURCE_KIND[kind],
      scope: getResourceCatalogEntry(kind).scope,
      verbs: kind === 'httproute' && updateAllowed ? ['get', 'list', 'update'] : [],
    })),
    operations: CONTROLLER_ACCESS_OPERATIONS.map((name) => ({ name, allowed: false })),
  }
}

function createWrapper() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return {
    client,
    wrapper: ({ children }: { children: ReactNode }) => createElement(
      QueryClientProvider,
      { client },
      children,
    ),
  }
}

afterEach(() => {
  vi.restoreAllMocks()
})

describe('useControllerAccess', () => {
  it('isolates direct and selected Controller caches', () => {
    expect(controllerAccessQueryKey(null)).toEqual(['controller-access', 'direct'])
    expect(controllerAccessQueryKey('cluster-a/controller-1')).toEqual([
      'controller-access',
      'cluster-a/controller-1',
    ])
    expect(normalizeControllerAccessId('cluster-a~controller-1')).toBe('cluster-a/controller-1')
    expect(controllerAccessQueryKey('cluster-a~controller-1')).toEqual([
      'controller-access',
      'cluster-a/controller-1',
    ])
  })

  it('fetches the decoded Controller identity for a route-safe id', async () => {
    const get = vi.spyOn(controllerAccessApi, 'get').mockResolvedValue(accessDocument(true))
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useControllerAccess('cluster-a~controller-1'), { wrapper })

    await waitFor(() => expect(result.current.authorizationPending).toBe(false))
    expect(get).toHaveBeenCalledWith('cluster-a/controller-1')
  })

  it('stops using stale authorization during a refetch and after its failure', async () => {
    let rejectRefetch: ((reason: Error) => void) | undefined
    vi.spyOn(controllerAccessApi, 'get')
      .mockResolvedValueOnce(accessDocument(true))
      .mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectRefetch = reject }))
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useControllerAccess('cluster-a/controller-1'), { wrapper })

    await waitFor(() => expect(result.current.canResource('httproute', 'update')).toBe(true))
    act(() => { void result.current.refetch() })
    await waitFor(() => expect(result.current.authorizationPending).toBe(true))
    expect(result.current.data).toBeUndefined()
    expect(result.current.canResource('httproute', 'update')).toBe(false)

    act(() => rejectRefetch?.(new Error('connection lost')))
    await waitFor(() => expect(result.current.isError).toBe(true))
    expect(result.current.data).toBeUndefined()
    expect(result.current.canResource('httproute', 'update')).toBe(false)
  })

  it('observes revoked access after a successful refresh', async () => {
    vi.spyOn(controllerAccessApi, 'get')
      .mockResolvedValueOnce(accessDocument(true))
      .mockResolvedValueOnce(accessDocument(false))
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useControllerAccess('cluster-a/controller-1'), { wrapper })

    await waitFor(() => expect(result.current.canResource('httproute', 'update')).toBe(true))
    await act(async () => { await result.current.refetch() })
    await waitFor(() => expect(result.current.authorizationPending).toBe(false))
    expect(result.current.canResource('httproute', 'update')).toBe(false)
  })

  it('fetches a distinct snapshot when the selected Controller changes', async () => {
    const get = vi.spyOn(controllerAccessApi, 'get')
      .mockResolvedValueOnce(accessDocument(true))
      .mockResolvedValueOnce(accessDocument(false))
    const { wrapper } = createWrapper()
    const { result, rerender } = renderHook(
      ({ id }) => useControllerAccess(id),
      { initialProps: { id: 'cluster-a/controller-1' }, wrapper },
    )

    await waitFor(() => expect(result.current.canResource('httproute', 'update')).toBe(true))
    rerender({ id: 'cluster-b/controller-2' })
    await waitFor(() => expect(get).toHaveBeenLastCalledWith('cluster-b/controller-2'))
    await waitFor(() => expect(result.current.authorizationPending).toBe(false))
    expect(result.current.canResource('httproute', 'update')).toBe(false)
  })
})
