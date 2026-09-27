import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import GRPCMethodMatchEditor from './GRPCMethodMatchEditor'

describe('gRPC optional method predicates', () => {
  it('clears method and service independently while retaining headers and sibling matches', () => {
    const onChange = vi.fn()
    const headers = [{ name: 'x-tenant', value: 'one' }]
    const sibling = { method: { service: 'other.Service', method: 'Call' } }
    const initial = { method: { type: 'Exact' as const, service: 'demo.Service', method: 'Get' }, headers, future: false }
    const { rerender } = render(<GRPCMethodMatchEditor value={[initial, sibling]} onChange={onChange} />)
    fireEvent.change(screen.getAllByRole('textbox', { name: 'gRPC Method' })[0], { target: { value: '' } })
    const serviceOnly = { ...initial, method: { type: 'Exact' as const, service: 'demo.Service' } }
    expect(onChange).toHaveBeenLastCalledWith([serviceOnly, sibling])
    rerender(<GRPCMethodMatchEditor value={[serviceOnly, sibling]} onChange={onChange} />)
    fireEvent.change(screen.getAllByRole('textbox', { name: 'gRPC Service' })[0], { target: { value: '' } })
    expect(onChange).toHaveBeenLastCalledWith([{ headers, future: false }, sibling])
    rerender(<GRPCMethodMatchEditor value={[initial]} onChange={onChange} />)
    fireEvent.change(screen.getByRole('textbox', { name: 'gRPC Service' }), { target: { value: '' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...initial, method: { type: 'Exact', method: 'Get' } }])
  })

  it('preserves unknown method fields on a narrow edit', () => {
    const onChange = vi.fn()
    const value = [{ method: { service: 'demo.Service', future: { retained: true } } }]
    render(<GRPCMethodMatchEditor value={value} onChange={onChange} />)
    fireEvent.change(screen.getByRole('textbox', { name: 'gRPC Service' }), { target: { value: '' } })
    expect(onChange).toHaveBeenCalledWith([{ method: { future: { retained: true } } }])
  })

  it('allows deleting the last match and adds a matcher without invalid empty names', () => {
    const onChange = vi.fn()
    const { rerender } = render(<GRPCMethodMatchEditor value={[{}]} onChange={onChange} />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    expect(onChange).toHaveBeenLastCalledWith([])
    rerender(<GRPCMethodMatchEditor value={[]} onChange={onChange} />)
    fireEvent.click(screen.getByRole('button', { name: 'Add Match' }))
    expect(onChange).toHaveBeenLastCalledWith([{}])
    rerender(<GRPCMethodMatchEditor value={[{}]} onChange={onChange} disabled />)
    expect(screen.queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'gRPC Method' })).toBeDisabled()
  })
})
