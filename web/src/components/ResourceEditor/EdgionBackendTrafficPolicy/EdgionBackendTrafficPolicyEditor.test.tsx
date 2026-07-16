import * as yaml from 'js-yaml'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import EdgionBackendTrafficPolicyEditor from './EdgionBackendTrafficPolicyEditor'

const create = vi.fn()
const update = vi.fn()

vi.mock('@/api/resources', () => ({
  resourceApi: {
    create: (...args: unknown[]) => create(...args),
    update: (...args: unknown[]) => update(...args),
  },
}))

vi.mock('@/components/resource/PermissionAwareButton', () => ({
  default: (props: any) => {
    const buttonProps = { ...props }
    delete buttonProps.resourceKind
    delete buttonProps.resourceVerb
    delete buttonProps.loading
    return <button {...buttonProps} />
  },
}))

vi.mock('./EdgionBackendTrafficPolicyForm', () => ({
  default: ({ data, onChange }: any) => (
    <button
      onClick={() => onChange({
        ...data,
        metadata: {
          name: 'payments-policy',
          namespace: 'prod',
          resourceVersion: 'server-owned',
          creationTimestamp: '2026-07-15T00:00:00Z',
        },
        spec: {
          targetRefs: [{ group: '', kind: 'Service', name: 'payments' }],
          futureSpec: { preserved: true },
        },
        status: { conditions: [{ type: 'Accepted', status: 'True' }] },
      })}
    >
      Fill valid policy
    </button>
  ),
}))

vi.mock('@/components/YamlEditor', () => ({
  default: ({ value, onChange }: { value: string; onChange: (value: string) => void }) => (
    <textarea aria-label="Policy YAML" value={value} onChange={(event) => onChange(event.target.value)} />
  ),
}))

function renderEditor(props: React.ComponentProps<typeof EdgionBackendTrafficPolicyEditor>) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <EdgionBackendTrafficPolicyEditor {...props} />
    </QueryClientProvider>,
  )
}

describe('EdgionBackendTrafficPolicyEditor mutation paths', () => {
  beforeEach(() => {
    create.mockReset()
    update.mockReset()
    create.mockResolvedValue({ success: true })
    update.mockResolvedValue({ success: true })
  })

  it('creates a Form document through the common mutation boundary', async () => {
    renderEditor({ visible: true, mode: 'create', onClose: vi.fn() })

    fireEvent.click(screen.getByRole('button', { name: 'Fill valid policy' }))
    fireEvent.click(screen.getByTestId('editor-submit'))

    await waitFor(() => expect(create).toHaveBeenCalledOnce())
    expect(create.mock.calls[0].slice(1, 3)).toEqual(['edgionbackendtrafficpolicy', 'prod'])
    const payload = yaml.load(create.mock.calls[0][3]) as any
    expect(payload.metadata).toEqual({ name: 'payments-policy', namespace: 'prod' })
    expect(payload.status).toBeUndefined()
    expect(payload.spec.futureSpec).toEqual({ preserved: true })
  })

  it('updates an edited YAML document through the same mutation boundary', async () => {
    const resource = {
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionBackendTrafficPolicy',
      metadata: { name: 'payments-policy', namespace: 'prod', resourceVersion: 'old' },
      spec: { targetRefs: [{ kind: 'Service', name: 'payments' }] },
    }
    renderEditor({ visible: true, mode: 'edit', resource, onClose: vi.fn() })
    fireEvent.click(screen.getByRole('tab', { name: 'YAML' }))
    fireEvent.change(screen.getByLabelText('Policy YAML'), {
      target: {
        value: yaml.dump({
          ...resource,
          metadata: { ...resource.metadata, resourceVersion: 'new', managedFields: [{ manager: 'server' }] },
          spec: {
            ...resource.spec,
            loadBalancer: { type: 'LeastConn', panicThreshold: 0 },
            futureSpec: { preserved: false },
          },
          status: { conditions: [{ type: 'Accepted', status: 'True' }] },
        }),
      },
    })
    fireEvent.click(screen.getByTestId('editor-submit'))

    await waitFor(() => expect(update).toHaveBeenCalledOnce())
    expect(update.mock.calls[0].slice(1, 4)).toEqual([
      'edgionbackendtrafficpolicy', 'prod', 'payments-policy',
    ])
    const payload = yaml.load(update.mock.calls[0][4]) as any
    expect(payload.metadata).toEqual({ name: 'payments-policy', namespace: 'prod', resourceVersion: 'new' })
    expect(payload.status).toBeUndefined()
    expect(payload.spec.loadBalancer).toEqual({ type: 'LeastConn', panicThreshold: 0 })
    expect(payload.spec.futureSpec).toEqual({ preserved: false })
  })
})
