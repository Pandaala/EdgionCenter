import * as yaml from 'js-yaml'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import EdgionPluginsEditor from './EdgionPluginsEditor'
import EdgionStreamPluginsEditor from '../EdgionStreamPlugins/EdgionStreamPluginsEditor'
import EdgionConfigDataEditor from '../EdgionConfigData/EdgionConfigDataEditor'

const create = vi.fn()
const update = vi.fn()
vi.mock('@/api/resources', () => ({ resourceApi: {
  create: (...args: unknown[]) => create(...args),
  update: (...args: unknown[]) => update(...args),
} }))
vi.mock('@/components/resource/PermissionAwareButton', () => ({ default: (props: any) => {
  const buttonProps = { ...props }; delete buttonProps.resourceKind; delete buttonProps.resourceVerb; delete buttonProps.loading
  return <button {...buttonProps} />
} }))
vi.mock('@/components/YamlEditor', () => ({ default: ({ value, onChange }: any) => (
  <textarea aria-label="Resource YAML" value={value} onChange={(event) => onChange(event.target.value)} />
) }))
vi.mock('./EdgionPluginsForm', () => ({ default: ({ value, onChange }: any) => (
  <button onClick={() => onChange({
    ...value,
    metadata: { name: 'http-plugins', namespace: 'edge', resourceVersion: 'server' },
    spec: { requestPlugins: [{ type: 'Mock', config: { statusCode: 200, future: false } }], futureSpec: [] },
    status: { conditions: [] },
  })}>Fill HTTP plugins</button>
) }))
vi.mock('../EdgionStreamPlugins/EdgionStreamPluginsForm', () => ({ default: ({ data, onChange }: any) => (
  <button onClick={() => onChange({
    ...data,
    metadata: { name: 'stream-plugins', namespace: 'edge', resourceVersion: 'server' },
    spec: {
      plugins: [{ type: 'ConnectionRateLimit', config: { perListener: { rate: 10, interval: '1s' } } }],
      tlsRoutePlugins: [{ type: 'IpRestriction', config: { allow: [{ name: 'tls', cidrs: ['10.0.0.0/8'] }] } }],
      futureSpec: false,
    },
    status: { conditions: [] },
  })}>Fill Stream plugins</button>
) }))
vi.mock('../EdgionConfigData/EdgionConfigDataForm', () => ({ default: ({ data, onChange }: any) => (
  <button onClick={() => onChange({
    ...data,
    metadata: { name: 'config-data', namespace: 'edge', resourceVersion: 'server' },
    spec: { enable: true, visibility: 'Cluster', data: { type: 'Selector', config: { active: 'safe', future: 0 } }, futureSpec: '' },
    status: { conditions: [] },
  })}>Fill ConfigData</button>
) }))

function renderWithQuery(element: React.ReactElement) {
  return render(<QueryClientProvider client={new QueryClient({ defaultOptions: { mutations: { retry: false } } })}>{element}</QueryClientProvider>)
}

async function assertMutation(fn: ReturnType<typeof vi.fn>, kind: string, expectedSpec: (spec: any) => void) {
  await waitFor(() => expect(fn).toHaveBeenCalledOnce())
  expect(fn.mock.calls[0][1]).toBe(kind)
  const source = fn === create ? fn.mock.calls[0][3] : fn.mock.calls[0][4]
  const payload: any = yaml.load(source)
  expect(payload.metadata.resourceVersion).toBeUndefined()
  expect(payload.status).toBeUndefined()
  expectedSpec(payload.spec)
}

describe('plugin editor Form and YAML mutation boundaries', () => {
  beforeEach(() => {
    cleanup(); create.mockReset(); update.mockReset()
    create.mockResolvedValue({ success: true }); update.mockResolvedValue({ success: true })
  })

  it('submits EdgionPlugins Form-create and YAML-update through the same boundary', async () => {
    const first = renderWithQuery(<EdgionPluginsEditor visible mode="create" onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Fill HTTP plugins' }))
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await assertMutation(create, 'edgionplugins', (spec) => expect(spec.futureSpec).toEqual([]))
    first.unmount(); create.mockReset()

    const resource: any = { apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins', metadata: { name: 'http-plugins', namespace: 'edge' }, spec: { requestPlugins: [] } }
    renderWithQuery(<EdgionPluginsEditor visible mode="edit" resource={resource} onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Resource YAML'), { target: { value: yaml.dump({ ...resource, spec: { requestPlugins: [{ type: 'TraceContext', config: { trustInbound: true } }], futureSpec: false }, status: {} }) } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await assertMutation(update, 'edgionplugins', (spec) => expect(spec.futureSpec).toBe(false))
  })

  it('submits EdgionStreamPlugins Form-create and YAML-update through the same boundary', async () => {
    const first = renderWithQuery(<EdgionStreamPluginsEditor visible mode="create" onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Fill Stream plugins' }))
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await assertMutation(create, 'edgionstreamplugins', (spec) => {
      expect(spec.plugins).toHaveLength(1); expect(spec.tlsRoutePlugins).toHaveLength(1)
    })
    first.unmount(); create.mockReset()

    const resource: any = { apiVersion: 'edgion.io/v1', kind: 'EdgionStreamPlugins', metadata: { name: 'stream-plugins', namespace: 'edge' }, spec: { plugins: [] } }
    renderWithQuery(<EdgionStreamPluginsEditor visible mode="edit" resource={resource} onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Resource YAML'), { target: { value: yaml.dump({ ...resource, spec: { plugins: [], tlsRoutePlugins: [{ type: 'IpRestriction', config: { denyRefs: [{ name: 'blocked' }] } }], futureSpec: [] }, status: {} }) } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await assertMutation(update, 'edgionstreamplugins', (spec) => expect(spec.futureSpec).toEqual([]))
  })

  it('submits EdgionConfigData Form-create and YAML-update through the same boundary', async () => {
    const first = renderWithQuery(<EdgionConfigDataEditor visible mode="create" onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Fill ConfigData' }))
    fireEvent.click(screen.getByRole('button', { name: 'Create' }))
    await assertMutation(create, 'edgionconfigdata', (spec) => expect(spec.data.config.future).toBe(0))
    first.unmount(); create.mockReset()

    const resource: any = { apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData', metadata: { name: 'config-data', namespace: 'edge' }, spec: { enable: true, visibility: 'Namespace', data: { type: 'Misc', config: {} } } }
    renderWithQuery(<EdgionConfigDataEditor visible mode="edit" resource={resource} onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Resource YAML'), { target: { value: yaml.dump({ ...resource, spec: { ...resource.spec, data: { type: 'IpList', config: { items: [], future: false } }, futureSpec: 0 }, status: {} }) } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await assertMutation(update, 'edgionconfigdata', (spec) => expect(spec.futureSpec).toBe(0))
  })
})
