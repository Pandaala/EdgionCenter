import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import ResourceStatus from './ResourceStatus'
import { useInvalidateRuntimeStatus } from '@/hooks/useRuntimeResourceStatus'

const target = vi.hoisted(() => ({ controllerId: 'east/controller' as string | null }))
vi.mock('@/hooks/useControllerMutationTarget', () => ({ useControllerMutationTarget: () => ({ ...target }) }))
const source = (name = 'route'): K8sResource => ({
  apiVersion: 'gateway.networking.k8s.io/v1', kind: 'HTTPRoute',
  metadata: { name, namespace: 'app', resourceVersion: '7' },
  spec: { hostnames: ['example.com'] },
})
const status = (value = 'True') => ({ parents: [{ parentRef: { name: 'gateway' }, conditions: [
  { type: 'ResolvedRefs', status: value, reason: value === 'True' ? 'ResolvedRefs' : 'RefNotPermitted' },
] }] })
let client: QueryClient
beforeEach(() => { target.controllerId = 'east/controller'; client = new QueryClient({ defaultOptions: { queries: { retry: false } } }) })
afterEach(() => { cleanup(); client.clear(); vi.restoreAllMocks() })
const wrap = (element: React.ReactNode) => <QueryClientProvider client={client}>{element}</QueryClientProvider>

function Cell({ resource, revision = 0 }: { resource: K8sResource; revision?: number }) {
  useInvalidateRuntimeStatus('httproute', revision)
  return <ResourceStatus kind="httproute" resource={resource} />
}

describe('ResourceStatus', () => {
  it('preserves source multi-writer conditions without requesting or substituting processed status', () => {
    const read = vi.spyOn(resourceApi, 'getProcessed')
    const original = { ...source(), status: { controllers: [
      { controllerName: 'east', status: status('True') }, { controllerName: 'west', status: status('False') },
    ] } }
    render(wrap(<Cell resource={original} />))
    expect(screen.getByText('ResolvedRefs=True')).toBeInTheDocument()
    expect(screen.getByText('ResolvedRefs=False')).toBeInTheDocument()
    expect(read).not.toHaveBeenCalled()
  })

  it('observes the matching version while keeping processed spec out of the source and query cache', async () => {
    const original = source()
    const read = vi.spyOn(resourceApi, 'getProcessed').mockResolvedValue({ ...original, spec: { runtimeOnly: 'private' }, status: status() })
    render(wrap(<Cell resource={original} />))
    expect(await screen.findByText('ResolvedRefs=True')).toBeInTheDocument()
    expect(read).toHaveBeenCalledWith({ controllerId: 'east/controller' }, 'httproute', 'app', 'route', expect.any(AbortSignal))
    expect(original).toEqual(source())
    expect(JSON.stringify(client.getQueryCache().getAll().map(q => q.state.data))).not.toContain('runtimeOnly')
  })

  it.each(['version', 'name', 'namespace', 'kind'] as const)('rejects a mismatched observation: %s', async (field) => {
    const observed = { ...source(), metadata: { ...source().metadata }, status: status() }
    if (field === 'version') observed.metadata.resourceVersion = '6'
    else if (field === 'kind') observed.kind = 'GRPCRoute'
    else observed.metadata[field] = 'different'
    vi.spyOn(resourceApi, 'getProcessed').mockResolvedValue(observed)
    render(wrap(<Cell resource={source()} />))
    expect(await screen.findByText('Status awaiting processing')).toBeInTheDocument()
    expect(screen.queryByText('ResolvedRefs=True')).not.toBeInTheDocument()
  })

  it('hides old healthy status on failed refresh and recovers after source refresh without a version change', async () => {
    const read = vi.spyOn(resourceApi, 'getProcessed').mockResolvedValue({ ...source(), status: status() })
    const view = render(wrap(<Cell resource={source()} revision={1} />))
    expect(await screen.findByText('ResolvedRefs=True')).toBeInTheDocument()
    read.mockRejectedValue(new Error('sensitive remote detail'))
    view.rerender(wrap(<Cell resource={source()} revision={2} />))
    expect(await screen.findByText('Status unavailable')).toBeInTheDocument()
    expect(screen.queryByText('ResolvedRefs=True')).not.toBeInTheDocument()
    expect(screen.queryByText(/sensitive/)).not.toBeInTheDocument()
    read.mockResolvedValue({ ...source(), status: status('False') })
    view.rerender(wrap(<Cell resource={source()} revision={3} />))
    expect(await screen.findByTestId('route-ref-denied')).toBeInTheDocument()
  })

  it('does not reuse another Controller observation for the same resource identity and version', async () => {
    const read = vi.spyOn(resourceApi, 'getProcessed').mockResolvedValue({ ...source(), status: status() })
    const view = render(wrap(<Cell resource={source()} />))
    expect(await screen.findByText('ResolvedRefs=True')).toBeInTheDocument()
    target.controllerId = 'west/controller'
    read.mockResolvedValue({ ...source(), status: status('False') })
    view.rerender(wrap(<Cell resource={source()} />))
    expect(await screen.findByTestId('route-ref-denied')).toBeInTheDocument()
    expect(screen.queryByText('ResolvedRefs=True')).not.toBeInTheDocument()
    expect(read.mock.calls.at(-1)?.[0]).toEqual({ controllerId: 'west/controller' })
  })

  it('does not fetch or claim freshness without a source version', () => {
    const read = vi.spyOn(resourceApi, 'getProcessed')
    const original = source(); delete original.metadata.resourceVersion
    render(wrap(<Cell resource={original} />))
    expect(screen.getByText('Status unavailable')).toBeInTheDocument()
    expect(read).not.toHaveBeenCalled()
  })

  it('bounds concurrent reads and cancels queued resources when the page unmounts', async () => {
    const finish: Array<() => void> = []
    const read = vi.spyOn(resourceApi, 'getProcessed').mockImplementation(async (_target, _kind, _namespace, name) => {
      await new Promise<void>(resolve => finish.push(resolve))
      return { ...source(name), status: status() }
    })
    const view = render(wrap(<>{Array.from({ length: 9 }, (_, i) => <Cell key={i} resource={source(String(i))} />)}</>))
    await waitFor(() => expect(read).toHaveBeenCalledTimes(4))
    view.unmount()
    await act(async () => { finish.forEach(done => done()) })
    expect(read).toHaveBeenCalledTimes(4)
  })
})
