import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import GatewayTLSSection from './GatewayTLSSection'

describe('GatewayTLSSection', () => {
  it('edits one per-port override without deleting backend, default, sibling ports, or unknown fields', () => {
    const onChange = vi.fn()
    const value: any = {
      backend: { clientCertificateRef: { name: 'client', namespace: 'certs' }, futureBackend: false },
      frontend: {
        default: { validation: { mode: 'AllowValidOnly', caCertificateRefs: [{ name: 'default-ca' }] }, futureDefault: [] },
        perPort: [
          { port: 443, tls: { validation: { mode: 'AllowValidOnly', caCertificateRefs: [{ name: 'one' }] } }, futurePort: true },
          { port: 8443, tls: { validation: { mode: 'AllowInsecureFallback', caCertificateRefs: [{ name: 'two' }] } }, futurePort: false },
        ],
        futureFrontend: '',
      },
      futureTls: { preserved: true },
    }
    render(<GatewayTLSSection value={value} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue('8443'), { target: { value: '9443' } })
    const next = onChange.mock.calls[onChange.mock.calls.length - 1][0]
    expect(next.backend).toEqual(value.backend)
    expect(next.frontend.default).toEqual(value.frontend.default)
    expect(next.frontend.perPort[0]).toEqual(value.frontend.perPort[0])
    expect(next.frontend.perPort[1]).toEqual({ ...value.frontend.perPort[1], port: 9443 })
    expect(next.frontend.futureFrontend).toBe('')
    expect(next.futureTls).toEqual({ preserved: true })
  })
})

it('adds the required empty default when starting with a per-port policy', () => {
  const onChange = vi.fn()
  const value = { backend: { clientCertificateRef: { name: 'client' } } }
  render(<GatewayTLSSection value={value} onChange={onChange} />)
  fireEvent.click(screen.getByRole('button', { name: /Add Port Override/i }))
  expect(onChange).toHaveBeenCalledWith({
    ...value, frontend: { default: {}, perPort: [{ port: 443, tls: {
      validation: { mode: 'AllowValidOnly', caCertificateRefs: [] },
    } }] },
  })
})


it.each(['default', 'port'])('clears %s validation while preserving other TLS configuration', target => {
  const onChange = vi.fn()
  const validation = { mode: 'AllowValidOnly', caCertificateRefs: [{ group: '', kind: 'Secret', name: 'ca' }] }
  const value: any = {
    backend: { clientCertificateRef: { name: 'backend-client' } },
    frontend: {
      default: { validation, futureDefault: false },
      perPort: [
        { port: 443, tls: { validation, futureTls: false } },
        { port: 8443, tls: { validation } },
      ],
    },
  }
  render(<GatewayTLSSection value={value} onChange={onChange} />)
  const buttons = screen.getAllByRole('button', { name: 'Clear Frontend Client Certificate Validation' })
  fireEvent.click(buttons[target === 'default' ? 0 : 1])
  const next = onChange.mock.calls[0][0]
  expect(next.backend).toEqual(value.backend)
  expect(next.frontend.perPort[1]).toEqual(value.frontend.perPort[1])
  if (target === 'default') {
    expect(next.frontend.default).toEqual({ validation: undefined, futureDefault: false })
    expect(next.frontend.perPort).toEqual(value.frontend.perPort)
  } else {
    expect(next.frontend.default).toEqual(value.frontend.default)
    expect(next.frontend.perPort[0]).toEqual({ port: 443, tls: { validation: undefined, futureTls: false } })
  }
})
