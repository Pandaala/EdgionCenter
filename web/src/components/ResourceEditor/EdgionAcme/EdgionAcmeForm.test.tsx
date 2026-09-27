import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { createEmpty } from '@/utils/edgionacme'
import EdgionAcmeForm from './EdgionAcmeForm'

describe('EdgionAcmeForm', () => {
  it('shows the supported challenge and preserves listener scoping during a Gateway edit', () => {
    const resource = createEmpty()
    resource.spec.challenge.gatewayRef = {
      name: 'old-gateway', namespace: 'edge', sectionName: 'http', port: 80,
      group: 'gateway.networking.k8s.io', kind: 'Gateway',
    }
    resource.spec.externalAccountBinding = { keyId: 'issuer-id', keySecretRef: { name: 'eab' } }
    const onChange = vi.fn()
    render(<EdgionAcmeForm data={resource} onChange={onChange} />)
    expect(screen.getByDisplayValue('http-01')).toHaveAttribute('readonly')
    expect(screen.queryByText('dns-01')).not.toBeInTheDocument()
    fireEvent.change(screen.getByDisplayValue('old-gateway'), { target: { value: 'new-gateway' } })
    expect(onChange.mock.calls.at(-1)?.[0].spec).toEqual({
      ...resource.spec,
      challenge: { type: 'http-01', gatewayRef: { ...resource.spec.challenge.gatewayRef, name: 'new-gateway' } },
    })
  })
})
