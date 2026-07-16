import * as yaml from 'js-yaml'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import GatewayEditor from './Gateway/GatewayEditor'
import GatewayClassEditor from './GatewayClass/GatewayClassEditor'
import EdgionGatewayConfigEditor from './EdgionGatewayConfig/EdgionGatewayConfigEditor'

const resourceCreate = vi.fn()
const resourceUpdate = vi.fn()
const clusterCreate = vi.fn()
const clusterUpdate = vi.fn()

vi.mock('@/api/resources', () => ({
  resourceApi: {
    create: (...args: unknown[]) => resourceCreate(...args),
    update: (...args: unknown[]) => resourceUpdate(...args),
  },
  clusterResourceApi: {
    create: (...args: unknown[]) => clusterCreate(...args),
    update: (...args: unknown[]) => clusterUpdate(...args),
  },
}))

vi.mock('./Gateway/GatewayForm', () => ({
  default: ({ data, onChange }: any) => <button onClick={() => onChange({
    ...data,
    metadata: { name: 'edge', namespace: 'prod', resourceVersion: 'server', managedFields: [{}] },
    spec: {
      gatewayClassName: 'edgion',
      listeners: [{ name: 'http', port: 80, protocol: 'HTTP' }],
      tls: { backend: { clientCertificateRef: { name: 'client', namespace: 'certs' }, resolvedClientCertificate: '[redacted]' } },
      futureSpec: { preserved: true },
    },
    status: { conditions: [] },
  })}>Fill Gateway</button>,
}))

vi.mock('./GatewayClass/GatewayClassForm', () => ({
  default: ({ data, onChange }: any) => <button onClick={() => onChange({
    ...data,
    metadata: { name: 'edgion', resourceVersion: 'server' },
    spec: { controllerName: 'edgion.io/gateway-controller', parametersRef: { group: 'edgion.io', kind: 'EdgionGatewayConfig', name: 'default' }, futureSpec: [] },
    status: { supportedFeatures: [{ name: 'HTTPRoute' }] },
  })}>Fill GatewayClass</button>,
}))

vi.mock('./EdgionGatewayConfig/EdgionGatewayConfigForm', () => ({
  default: ({ data, onChange }: any) => <button onClick={() => onChange({
    ...data,
    metadata: { name: 'default', resourceVersion: 'server' },
    spec: {
      httpTimeout: { client: { readTimeout: '60s' } },
      realIp: { trustedIps: [{ name: 'private', cidrs: ['10.0.0.0/8'] }] },
      outboundTls: { verify: true, resolvedCaCertificates: '[redacted]', resolvedClientCertificate: '[redacted]' },
      futureSpec: { preserved: false },
    },
    status: { conditions: [] },
  })}>Fill Gateway Config</button>,
}))

vi.mock('@/components/YamlEditor', () => ({
  default: ({ value, onChange }: { value: string; onChange: (value: string) => void }) => <textarea aria-label="Editor YAML" value={value} onChange={(event) => onChange(event.target.value)} />,
}))

function renderEditor(element: React.ReactElement) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  return render(<QueryClientProvider client={client}>{element}</QueryClientProvider>)
}

describe('Gateway family editor submit boundaries', () => {
  beforeEach(() => {
    for (const mock of [resourceCreate, resourceUpdate, clusterCreate, clusterUpdate]) {
      mock.mockReset()
      mock.mockResolvedValue({ success: true })
    }
  })

  it('Gateway Form create validates then strips server/runtime fields', async () => {
    renderEditor(<GatewayEditor visible mode="create" onClose={vi.fn()} />)
    fireEvent.click(screen.getByTestId('editor-submit'))
    expect(resourceCreate).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Fill Gateway' }))
    fireEvent.click(screen.getByTestId('editor-submit'))
    await waitFor(() => expect(resourceCreate).toHaveBeenCalledOnce())
    expect(resourceCreate.mock.calls[0].slice(1, 3)).toEqual(['gateway', 'prod'])
    const payload = yaml.load(resourceCreate.mock.calls[0][3]) as any
    expect(payload.metadata).toEqual({ name: 'edge', namespace: 'prod' })
    expect(payload.status).toBeUndefined()
    expect(payload.spec.tls.backend.resolvedClientCertificate).toBeUndefined()
    expect(payload.spec.futureSpec).toEqual({ preserved: true })
  })

  it('Gateway YAML update blocks invalid global TLS and sends the valid safe document', async () => {
    const resource: any = { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'Gateway', metadata: { name: 'edge', namespace: 'prod' }, spec: { gatewayClassName: 'edgion', listeners: [{ name: 'http', port: 80, protocol: 'HTTP' }] } }
    renderEditor(<GatewayEditor visible mode="edit" resource={resource} onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Editor YAML'), { target: { value: yaml.dump({ ...resource, spec: { ...resource.spec, tls: { frontend: { perPort: [{ port: 0, tls: { validation: { caCertificateRefs: [] } } }] } } } }) } })
    fireEvent.click(screen.getByTestId('editor-submit'))
    expect(resourceUpdate).not.toHaveBeenCalled()
    const valid = { ...resource, metadata: { ...resource.metadata, resourceVersion: 'server' }, spec: { ...resource.spec, tls: { frontend: { perPort: [{ port: 443, tls: { validation: { caCertificateRefs: [{ name: 'ca', kind: 'ConfigMap' }] } } }] } }, futureSpec: [] }, status: {} }
    fireEvent.change(screen.getByLabelText('Editor YAML'), { target: { value: yaml.dump(valid) } })
    fireEvent.click(screen.getByTestId('editor-submit'))
    await waitFor(() => expect(resourceUpdate).toHaveBeenCalledOnce())
    const payload = yaml.load(resourceUpdate.mock.calls[0][4]) as any
    expect(payload.metadata).toEqual({ name: 'edge', namespace: 'prod', resourceVersion: 'server' })
    expect(payload.spec.futureSpec).toEqual([])
    expect(payload.status).toBeUndefined()
  })

  it('GatewayClass Form create validates and strips status', async () => {
    renderEditor(<GatewayClassEditor visible mode="create" onClose={vi.fn()} />)
    fireEvent.click(screen.getByTestId('editor-submit'))
    expect(clusterCreate).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Fill GatewayClass' }))
    fireEvent.click(screen.getByTestId('editor-submit'))
    await waitFor(() => expect(clusterCreate).toHaveBeenCalledOnce())
    const payload = yaml.load(clusterCreate.mock.calls[0][2]) as any
    expect(payload.metadata).toEqual({ name: 'edgion' })
    expect(payload.status).toBeUndefined()
    expect(payload.spec.futureSpec).toEqual([])
  })

  it('GatewayClass YAML update rejects namespace and accepts the current parametersRef', async () => {
    const resource: any = { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'GatewayClass', metadata: { name: 'edgion' }, spec: { controllerName: 'edgion.io/gateway-controller' } }
    renderEditor(<GatewayClassEditor visible mode="edit" resource={resource} onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Editor YAML'), { target: { value: yaml.dump({ ...resource, spec: { ...resource.spec, parametersRef: { group: 'edgion.io', kind: 'EdgionGatewayConfig', name: 'default', namespace: 'bad' } } }) } })
    fireEvent.click(screen.getByTestId('editor-submit'))
    expect(clusterUpdate).not.toHaveBeenCalled()
    fireEvent.change(screen.getByLabelText('Editor YAML'), { target: { value: yaml.dump({ ...resource, metadata: { ...resource.metadata, resourceVersion: 'server' }, spec: { ...resource.spec, parametersRef: { group: 'edgion.io', kind: 'EdgionGatewayConfig', name: 'default' } }, status: {} }) } })
    fireEvent.click(screen.getByTestId('editor-submit'))
    await waitFor(() => expect(clusterUpdate).toHaveBeenCalledOnce())
    const payload = yaml.load(clusterUpdate.mock.calls[0][3]) as any
    expect(payload.metadata).toEqual({ name: 'edgion', resourceVersion: 'server' })
    expect(payload.status).toBeUndefined()
  })

  it('GatewayConfig Form create validates and strips resolved TLS fields', async () => {
    renderEditor(<EdgionGatewayConfigEditor visible mode="create" onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Fill Gateway Config' }))
    fireEvent.click(screen.getByTestId('editor-submit'))
    await waitFor(() => expect(clusterCreate).toHaveBeenCalledOnce())
    const payload = yaml.load(clusterCreate.mock.calls[0][2]) as any
    expect(payload.metadata).toEqual({ name: 'default' })
    expect(payload.status).toBeUndefined()
    expect(payload.spec.outboundTls.resolvedCaCertificates).toBeUndefined()
    expect(payload.spec.outboundTls.resolvedClientCertificate).toBeUndefined()
    expect(payload.spec.futureSpec).toEqual({ preserved: false })
  })

  it('GatewayConfig YAML update blocks invalid DNS/duration and sends valid unknown fields', async () => {
    const resource: any = { apiVersion: 'edgion.io/v1alpha1', kind: 'EdgionGatewayConfig', metadata: { name: 'default' }, spec: {} }
    renderEditor(<EdgionGatewayConfigEditor visible mode="edit" resource={resource} onClose={vi.fn()} />)
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Editor YAML'), { target: { value: yaml.dump({ ...resource, spec: { tcpTimeout: { idleTimeout: 'later' }, dnsResolver: { servers: ['1.1.1.1'], linkSysRef: { namespace: 'prod', name: 'dns' } } } }) } })
    fireEvent.click(screen.getByTestId('editor-submit'))
    expect(clusterUpdate).not.toHaveBeenCalled()
    fireEvent.change(screen.getByLabelText('Editor YAML'), { target: { value: yaml.dump({ ...resource, metadata: { ...resource.metadata, resourceVersion: 'server' }, spec: { tcpTimeout: { idleTimeout: '1h' }, dnsResolver: { servers: ['1.1.1.1'], cacheTtl: '5s' }, pathNormalization: { legacyUnknownField: false } }, status: {} }) } })
    fireEvent.click(screen.getByTestId('editor-submit'))
    await waitFor(() => expect(clusterUpdate).toHaveBeenCalledOnce())
    const payload = yaml.load(clusterUpdate.mock.calls[0][3]) as any
    expect(payload.metadata).toEqual({ name: 'default', resourceVersion: 'server' })
    expect(payload.spec.pathNormalization).toEqual({ legacyUnknownField: false })
    expect(payload.status).toBeUndefined()
  })
})
