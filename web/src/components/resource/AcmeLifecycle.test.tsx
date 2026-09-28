import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { resourceApi } from '@/api/resources'
import type { K8sResource } from '@/api/types'
import AcmeLifecycle from './AcmeLifecycle'
import ResourceStatus from './ResourceStatus'

vi.mock('@/hooks/useControllerMutationTarget', () => ({ useControllerMutationTarget: () => ({ controllerId: 'east/controller' }) }))
const source: K8sResource = { apiVersion: 'edgion.io/v1', kind: 'EdgionAcme', metadata: { name: 'certificate', namespace: 'app', resourceVersion: '7' }, spec: {} }
const status = { phase: 'Ready', certificateNotAfter: '2026-12-31T23:59:00+08:00' }
let client: QueryClient
beforeEach(() => { client = new QueryClient({ defaultOptions: { queries: { retry: false } } }) })
afterEach(() => { cleanup(); client.clear(); vi.restoreAllMocks() })
const show = (resource: K8sResource) => render(<QueryClientProvider client={client}>
  <AcmeLifecycle resource={resource} /><ResourceStatus kind="edgionacme" resource={resource} />
</QueryClientProvider>)

it('shares a version-bound processed observation with Conditions and displays UTC expiry', async () => {
  const read = vi.spyOn(resourceApi, 'getProcessed').mockResolvedValue({ ...source, status })
  const { container } = show(source)
  expect(await screen.findByText('Ready')).toBeInTheDocument()
  expect(container.querySelector('time')).toHaveAttribute('datetime', '2026-12-31T15:59:00.000Z')
  expect(read).toHaveBeenCalledTimes(1)
  expect(source.status).toBeUndefined()
})

it('preserves source lifecycle without fetching a conflicting processed status', () => {
  const read = vi.spyOn(resourceApi, 'getProcessed')
  show({ ...source, status: { ...status, phase: 'Renewing' } })
  expect(screen.getByText('Renewing')).toBeInTheDocument()
  expect(read).not.toHaveBeenCalled()
})

it('does not fabricate expiry or a lifecycle phase from invalid or absent values', () => {
  const { container } = show({ ...source, status: { certificateNotAfter: 'invalid-date' } })
  expect(container.querySelector('time')).toBeNull()
  expect(screen.queryByText('Ready')).not.toBeInTheDocument()
  expect(screen.queryByText('Pending')).not.toBeInTheDocument()
})

it('rejects a mismatched version instead of showing an old certificate', async () => {
  vi.spyOn(resourceApi, 'getProcessed').mockResolvedValue({ ...source, metadata: { ...source.metadata, resourceVersion: '6' }, status })
  const { container } = show(source)
  await waitFor(() => expect(screen.getAllByText('Status awaiting processing')).toHaveLength(2))
  expect(screen.queryByText('Ready')).not.toBeInTheDocument()
  expect(container.querySelector('time')).toBeNull()
})

it('hides cached lifecycle and expiry after a failed observation refresh', async () => {
  const read = vi.spyOn(resourceApi, 'getProcessed').mockResolvedValue({ ...source, status })
  const { container } = show(source)
  expect(await screen.findByText('Ready')).toBeInTheDocument()
  read.mockRejectedValue(new Error('private remote detail'))
  await client.invalidateQueries({ queryKey: ['resource-runtime-status', 'edgionacme'] })
  await waitFor(() => expect(screen.getAllByText('Status unavailable')).toHaveLength(2))
  expect(screen.queryByText('Ready')).not.toBeInTheDocument()
  expect(container.querySelector('time')).toBeNull()
  expect(screen.queryByText(/private remote detail/)).not.toBeInTheDocument()
})
