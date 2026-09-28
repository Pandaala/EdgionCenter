import { createElement, type ReactNode } from 'react'
import { cleanup, fireEvent, renderHook, screen, waitFor } from '@testing-library/react'
import { QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { apiClient } from '@/api/client'
import { setActiveControllerId } from '@/utils/proxy'
import { createTestQueryClient, renderWithQueryClient } from '@/test/render'
import { TOPOLOGY_KINDS, useTopologyData } from '@/pages/Topology/hooks/useTopologyData'
import RestrictedDependenciesPage from './RestrictedDependenciesPage'

const route = vi.hoisted(() => ({ controllerId: 'east~controller' }))
vi.mock('react-router-dom', async () => ({ ...await vi.importActual('react-router-dom'), useParams: () => route }))
vi.mock('@/hooks/useControllerAccess', () => ({ useControllerAccess: () => ({ canResource: () => true }) }))
vi.mock('@/components/resource/PermissionAwareButton', () => ({ default: (props: any) => <button onClick={props.onClick}>{props.children}</button> }))
vi.mock('@/components/ResourceEditor/Secret/SecretEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/ConfigMap/ConfigMapEditor', () => ({ default: () => null }))
const originalAdapter = apiClient.defaults.adapter
const observed: Array<{ baseURL?: string; url?: string }> = []
beforeEach(() => {
  route.controllerId = 'east~controller'
  observed.length = 0
  setActiveControllerId('unrelated/controller')
  apiClient.defaults.adapter = async (config) => {
    observed.push({ baseURL: config.baseURL, url: config.url })
    const kind = config.url?.endsWith('/secret') ? 'Secret' : config.url?.endsWith('/configmap') ? 'ConfigMap' : 'Service'
    const name = config.baseURL?.includes('east~') ? 'east-only' : 'west-only'
    const data = config.url?.includes('/keys/') || config.url === '/namespaced/service'
      ? [{ apiVersion: 'v1', kind, metadata: { name, namespace: 'app' } }] : []
    return { config, data: { success: true, data, count: data.length }, status: 200, statusText: 'OK', headers: {} }
  }
})
afterEach(() => { cleanup(); apiClient.defaults.adapter = originalAdapter; setActiveControllerId(null) })

it('keeps Secret and ConfigMap metadata on the route Controller despite a different global target', async () => {
  const view = renderWithQueryClient(<RestrictedDependenciesPage />)
  expect(await screen.findByText('east-only')).toBeInTheDocument()
  fireEvent.click(screen.getByTestId('configmap-tab'))
  await waitFor(() => expect(observed).toHaveLength(2))
  expect(observed.every((r) => r.baseURL === '/api/v1/proxy/east~controller/api/v1')).toBe(true)
  route.controllerId = 'west~controller'
  view.rerender(<QueryClientProvider client={view.queryClient}><RestrictedDependenciesPage /></QueryClientProvider>)
  expect(screen.queryByText('east-only')).not.toBeInTheDocument()
  expect(await screen.findByText('west-only')).toBeInTheDocument()
  expect(observed.at(-1)).toEqual({ baseURL: '/api/v1/proxy/west~controller/api/v1', url: '/keys/namespaced/configmap' })
  view.unmount(); view.queryClient.clear()
})

it('pins every topology inventory request and isolates graph nodes after Controller switching', async () => {
  const client = createTestQueryClient()
  const wrapper = ({ children }: { children: ReactNode }) => createElement(QueryClientProvider, { client }, children)
  const view = renderHook(() => useTopologyData(null), { wrapper })
  await waitFor(() => expect(observed).toHaveLength(TOPOLOGY_KINDS.length))
  await waitFor(() => expect(view.result.current.nodes.some((n) => n.data.name === 'east-only')).toBe(true))
  expect(observed.every((r) => r.baseURL === '/api/v1/proxy/east~controller/api/v1')).toBe(true)
  route.controllerId = 'west~controller'
  view.rerender()
  expect(view.result.current.nodes).toEqual([])
  await waitFor(() => expect(observed).toHaveLength(TOPOLOGY_KINDS.length * 2))
  await waitFor(() => expect(view.result.current.nodes.some((n) => n.data.name === 'west-only')).toBe(true))
  expect(view.result.current.nodes.some((n) => n.data.name === 'east-only')).toBe(false)
  expect(observed.slice(TOPOLOGY_KINDS.length).every((r) => r.baseURL === '/api/v1/proxy/west~controller/api/v1')).toBe(true)
  expect(observed.filter((r) => r.url?.includes('/keys/'))).toHaveLength(4)
  view.unmount(); client.clear()
})
