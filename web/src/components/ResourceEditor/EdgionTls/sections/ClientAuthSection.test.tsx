import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import ClientAuthSection from './ClientAuthSection'
import type { ClientAuth } from '@/types/edgion-tls'

describe('ClientAuthSection', () => {
  it('edits typed SAN values without losing match policy, CNs or Secret identity', () => {
    const value: ClientAuth = { mode: 'Mutual', caSecretRef: { name: 'ca', namespace: 'pki', group: 'core', kind: 'Secret' }, allowedCns: ['client'], allowedSans: [{ type: 'OtherName', match: 'Prefix', value: 'before', oid: '1.2.3', ignoreCase: true }, { type: 'URI', value: 'spiffe://cluster/client' }] }
    const onChange = vi.fn()
    render(<ClientAuthSection value={value} onChange={onChange} />)
    fireEvent.change(screen.getByLabelText('SAN value 1'), { target: { value: 'after' } })
    expect(onChange.mock.calls.at(-1)?.[0]).toEqual({ ...value, allowedSans: [{ ...value.allowedSans![0], value: 'after' }, value.allowedSans![1]] })
    fireEvent.change(screen.getByDisplayValue('ca'), { target: { value: 'rotated-ca' } })
    expect(onChange.mock.calls.at(-1)?.[0].caSecretRef).toEqual({ ...value.caSecretRef, name: 'rotated-ca' })
  })
})
