import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import BackendTLSPolicyForm from './BackendTLSPolicyForm'
import { createEmpty } from '@/utils/backendtlspolicy'

describe('BackendTLSPolicy trust source controls', () => {
  it.each(['system', 'eight'] as const)('prevents adding CA refs with %s trust state', state => {
    const policy = createEmpty()
    if (state === 'system') {
      policy.spec.validation.wellKnownCACertificates = 'System'
      policy.spec.validation.caCertificateRefs = []
    } else {
      policy.spec.validation.caCertificateRefs = Array.from({ length: 8 }, (_, i) => ({ group: '', kind: 'ConfigMap', name: `ca-${i}` }))
    }
    const onChange = vi.fn()
    render(<BackendTLSPolicyForm data={policy} onChange={onChange} />)
    const add = screen.getByRole('button', { name: /Add CA/i })
    expect(add).toBeDisabled()
    fireEvent.click(add)
    expect(onChange).not.toHaveBeenCalled()
  })
})
