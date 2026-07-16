import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import StreamRouteForm from './StreamRouteForm'

vi.mock('../common/MetadataSection', () => ({ default: () => null }))
vi.mock('../common/ParentRefsSection', () => ({ default: () => null }))
vi.mock('../common/HostnamesSection', () => ({ default: () => null }))
vi.mock('./StreamAnnotationsSection', () => ({ default: () => null }))
vi.mock('../common/BackendRefsEditor', () => ({
  default: ({ onChange }: { onChange: (refs: unknown[]) => void }) => (
    <button onClick={() => onChange([{ name: 'changed', port: 9443 }])}>Change backends</button>
  ),
}))

describe('StreamRouteForm', () => {
  it('edits the selected rule without truncating sibling rules or hidden fields', () => {
    const onChange = vi.fn()
    const data = {
      apiVersion: 'gateway.networking.k8s.io/v1alpha2',
      kind: 'TCPRoute',
      metadata: { name: 'tcp', namespace: 'prod' },
      spec: {
        futureSpec: { enabled: false },
        rules: [
          { name: 'one', backendRefs: [{ name: 'first' }], futureRule: { keep: true } },
          { name: 'two', backendRefs: [{ name: 'second' }], futureRule: { untouched: true } },
        ],
      },
    }

    render(<StreamRouteForm kind="TCPRoute" data={data} onChange={onChange} />)
    fireEvent.click(screen.getByRole('button', { name: 'Change backends' }))

    expect(onChange).toHaveBeenCalledWith({
      ...data,
      spec: {
        ...data.spec,
        rules: [
          { name: 'one', backendRefs: [{ name: 'changed', port: 9443 }], futureRule: { keep: true } },
          data.spec.rules[1],
        ],
      },
    })
  })

  it('adds a rule without changing existing rules or spec extensions', () => {
    const onChange = vi.fn()
    const data = {
      apiVersion: 'gateway.networking.k8s.io/v1alpha2', kind: 'UDPRoute',
      metadata: { name: 'udp', namespace: 'prod' },
      spec: { futureSpec: true, rules: [{ backendRefs: [{ name: 'dns', port: 53 }], futureRule: true }] },
    }
    render(<StreamRouteForm kind="UDPRoute" data={data} onChange={onChange} />)
    fireEvent.click(screen.getByRole('button', { name: /Add Rule/ }))
    expect(onChange).toHaveBeenCalledWith({
      ...data,
      spec: { ...data.spec, rules: [data.spec.rules[0], { backendRefs: [{ name: '', port: 80, weight: 1 }] }] },
    })
  })
})
