import * as yaml from 'js-yaml'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import HTTPRouteEditor from './HTTPRoute/HTTPRouteEditor'
import GRPCRouteEditor from './GRPCRoute/GRPCRouteEditor'
import StreamRouteEditor from './StreamRoute/StreamRouteEditor'

const create = vi.fn()
const update = vi.fn()
const getProcessed = vi.fn()
vi.mock('@/api/resources', () => ({ resourceApi: {
  create: (...args: unknown[]) => create(...args),
  update: (...args: unknown[]) => update(...args),
  getProcessed: (...args: unknown[]) => getProcessed(...args),
} }))

vi.mock('@/components/YamlEditor', () => ({ default: ({ value, onChange }: { value: string; onChange: (value: string) => void }) =>
  <textarea aria-label="Route YAML" value={value} onChange={(e) => onChange(e.target.value)} /> }))

vi.mock('@/components/ResourceEditor/HTTPRoute/HTTPRouteForm', () => ({ default: ({ value, onChange }: any) =>
  <button onClick={() => onChange({ ...value, metadata: { name: 'http', namespace: 'edge', uid: 'server' }, spec: { ...value.spec, parentRefs: [{ name: 'gw' }], rules: [], futureSpec: true }, status: { runtime: true } })}>Fill HTTP</button> }))
vi.mock('@/components/ResourceEditor/GRPCRoute/GRPCRouteForm', () => ({ default: ({ data, onChange }: any) =>
  <button onClick={() => onChange({ ...data, metadata: { name: 'grpc', namespace: 'edge', uid: 'server' }, spec: { ...data.spec, parentRefs: [{ name: 'gw' }], futureSpec: true }, status: { runtime: true } })}>Fill GRPC</button> }))
vi.mock('@/components/ResourceEditor/StreamRoute/StreamRouteForm', () => ({ default: ({ data, onChange }: any) =>
  <button onClick={() => onChange({ ...data, metadata: { name: 'stream', namespace: 'edge', uid: 'server' }, spec: { ...data.spec, parentRefs: [{ name: 'gw' }], futureSpec: true }, status: { runtime: true } })}>Fill Stream</button> }))

const mount = (node: React.ReactNode) => render(<QueryClientProvider client={new QueryClient({ defaultOptions: { mutations: { retry: false } } })}>{node}</QueryClientProvider>)
const mutationPayload = () => yaml.load((create.mock.calls[0]?.[3] || update.mock.calls[0]?.[4]) as string) as any

describe('route Editor mutation paths', () => {
  beforeEach(() => {
    create.mockReset(); update.mockReset(); getProcessed.mockReset()
    create.mockResolvedValue({}); update.mockResolvedValue({}); getProcessed.mockResolvedValue({})
  })

  it('shows processed ReferenceGrant resolution in HTTPRoute details', async () => {
    const resource = { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'HTTPRoute', metadata: { name: 'allowed', namespace: 'edge' }, spec: { rules: [] } }
    getProcessed.mockResolvedValue({
      ...resource,
      status: { parents: [{ conditions: [{ type: 'ResolvedRefs', status: 'True', reason: 'ResolvedRefs' }] }] },
    })

    mount(<HTTPRouteEditor visible mode="view" resource={resource as any} onClose={vi.fn()} />)
    fireEvent.click(screen.getByTestId('editor-conditions-tab'))

    expect(await screen.findByTestId('route-ref-granted')).toHaveTextContent('ResolvedRefs=True')
    expect(getProcessed).toHaveBeenCalledWith(
      { controllerId: null },
      'httproute',
      'edge',
      'allowed',
    )
  })

  it.each([
    ['HTTP', <HTTPRouteEditor visible mode="create" onClose={vi.fn()} />, 'Fill HTTP', 'httproute'],
    ['GRPC', <GRPCRouteEditor visible mode="create" onClose={vi.fn()} />, 'Fill GRPC', 'grpcroute'],
    ['Stream', <StreamRouteEditor visible mode="create" kind="TCPRoute" onClose={vi.fn()} />, 'Fill Stream', 'tcproute'],
  ])('submits %s Form data through the mutation boundary', async (_label, editor, fillName, kind) => {
    mount(editor)
    fireEvent.click(screen.getByRole('button', { name: fillName }))
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await waitFor(() => expect(create).toHaveBeenCalledOnce())
    expect(create.mock.calls[0][1]).toBe(kind)
    const payload = mutationPayload()
    expect(payload.metadata.uid).toBeUndefined()
    expect(payload.status).toBeUndefined()
    expect(payload.spec.futureSpec).toBe(true)
  })

  it.each([
    ['HTTP', HTTPRouteEditor, { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'HTTPRoute', metadata: { name: 'http', namespace: 'edge' }, spec: { parentRefs: [{ name: 'gw' }], rules: [] } }],
    ['GRPC', GRPCRouteEditor, { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'GRPCRoute', metadata: { name: 'grpc', namespace: 'edge' }, spec: { parentRefs: [{ name: 'gw' }], rules: [] } }],
  ] as const)('submits %s YAML edits through the mutation boundary', async (label, Editor, resource) => {
    mount(<Editor visible mode="edit" resource={resource as any} onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Route YAML'), { target: { value: yaml.dump({ ...resource, metadata: { ...resource.metadata, managedFields: [{}] }, spec: { ...resource.spec, futureSpec: label }, status: { runtime: true } }) } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(update).toHaveBeenCalledOnce())
    const payload = mutationPayload()
    expect(payload.metadata.managedFields).toBeUndefined()
    expect(payload.status).toBeUndefined()
    expect(payload.spec.futureSpec).toBe(label)
  })

  it('submits Stream YAML edits through the mutation boundary', async () => {
    const resource: any = { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'TLSRoute', metadata: { name: 'tls', namespace: 'edge' }, spec: { parentRefs: [{ name: 'gw' }], rules: [] } }
    mount(<StreamRouteEditor visible mode="edit" kind="TLSRoute" resource={resource} onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Route YAML'), { target: { value: yaml.dump({ ...resource, metadata: { ...resource.metadata, uid: 'server' }, spec: { ...resource.spec, futureSpec: 'stream' }, status: { runtime: true } }) } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(update).toHaveBeenCalledOnce())
    const payload = mutationPayload()
    expect(payload.metadata.uid).toBeUndefined()
    expect(payload.status).toBeUndefined()
    expect(payload.spec.futureSpec).toBe('stream')
  })
})
