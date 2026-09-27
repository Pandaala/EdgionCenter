import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import BackendRefsEditor from './BackendRefsEditor'

describe('BackendRefsEditor weight', () => {
  it('preserves zero as an explicit disabled weight and retains sibling fields', () => {
    const onChange = vi.fn()
    const backend = { name: 'api', port: 8443, weight: 50, group: '', kind: 'Service', namespace: 'apps', futureField: false }
    const sibling = { name: 'fallback', port: 8443, weight: 50 }
    render(<BackendRefsEditor value={[backend, sibling]} onChange={onChange} />)

    const weight = screen.getAllByRole('spinbutton')[1]
    fireEvent.change(weight, { target: { value: '0' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...backend, weight: 0 }, sibling])

    fireEvent.change(weight, { target: { value: '' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...backend, weight: undefined }, sibling])
  })
})
