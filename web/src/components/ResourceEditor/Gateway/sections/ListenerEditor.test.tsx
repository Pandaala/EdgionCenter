import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import ListenerEditor from './ListenerEditor'

describe('ListenerEditor', () => {
  it('edits one certificate without deleting sibling refs or hidden TLS modules', () => {
    const onChange = vi.fn()
    const listener: any = {
      name: 'https',
      port: 443,
      protocol: 'HTTPS',
      allowedRoutes: { namespaces: { from: 'All' } },
      tls: {
        mode: 'Terminate',
        certificateRefs: [
          { group: '', kind: 'Secret', namespace: 'certs-a', name: 'one' },
          { group: 'cert-manager.io', kind: 'Certificate', namespace: 'certs-b', name: 'two' },
        ],
        options: { 'edgion.io/cert-provider': 'edgion-tls' },
        frontendValidation: { mode: 'AllowValidOnly', caCertificateRefs: [{ name: 'client-ca' }] },
      },
    }

    render(
      <ListenerEditor
        listener={listener}
        index={0}
        canRemove={false}
        onChange={onChange}
        onRemove={vi.fn()}
      />,
    )
    fireEvent.change(screen.getByDisplayValue('one'), { target: { value: 'one-updated' } })

    expect(onChange).toHaveBeenCalledWith({
      ...listener,
      tls: {
        ...listener.tls,
        certificateRefs: [
          { group: '', kind: 'Secret', namespace: 'certs-a', name: 'one-updated' },
          listener.tls.certificateRefs[1],
        ],
      },
    })
  })

  it('edits a selector label without collapsing kinds, certificates, or unknown listener fields', () => {
    const onChange = vi.fn()
    const listener: any = {
      name: 'https', port: 443, protocol: 'HTTPS', futureListener: { enabled: false },
      allowedRoutes: {
        namespaces: { from: 'Selector', selector: { matchLabels: { tenant: 'blue', environment: 'prod' }, matchExpressions: [{ key: 'tier', operator: 'In', values: ['edge'], futureExpression: false }], futureSelector: true } },
        kinds: [{ group: 'gateway.networking.k8s.io', kind: 'HTTPRoute' }, { kind: 'GRPCRoute' }],
      },
      tls: { mode: 'Terminate', certificateRefs: [{ name: 'one' }, { name: 'two' }], options: { custom: 'keep' } },
    }

    render(<ListenerEditor listener={listener} index={0} canRemove={false} onChange={onChange} onRemove={vi.fn()} />)
    fireEvent.change(screen.getByDisplayValue('blue'), { target: { value: 'green' } })

    const next = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0]
    expect(next.allowedRoutes.namespaces.selector).toEqual({ matchLabels: { tenant: 'green', environment: 'prod' }, matchExpressions: [{ key: 'tier', operator: 'In', values: ['edge'], futureExpression: false }], futureSelector: true })
    expect(next.allowedRoutes.kinds).toHaveLength(2)
    expect(next.tls.certificateRefs).toHaveLength(2)
    expect(next.futureListener).toEqual({ enabled: false })
  })

  it('accepts a custom domain-prefixed protocol in the structured form', () => {
    const onChange = vi.fn()
    const listener: any = { name: 'custom', port: 9443, protocol: 'HTTPS', tls: { mode: 'Terminate', options: { provider: 'external' } } }
    render(<ListenerEditor listener={listener} index={0} canRemove={false} onChange={onChange} onRemove={vi.fn()} />)
    fireEvent.change(screen.getByDisplayValue('HTTPS'), { target: { value: 'example.io/QUIC' } })
    expect(onChange).toHaveBeenCalledWith({ ...listener, protocol: 'example.io/QUIC' })
  })
})
