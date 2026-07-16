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
