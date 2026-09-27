import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import BackendRefsEditor from './BackendRefsEditor'

vi.mock('./RouteFiltersEditor', () => ({ default: () => null }))

describe('HTTP backend references', () => {
  it('switches to an AI backend without replaying Service port or losing unknown fields', () => {
    const onChange = vi.fn()
    const sibling = { name: 'secondary', kind: 'EdgionBackend', group: 'edgion.io', weight: 0 }
    render(<BackendRefsEditor value={[{ name: 'primary', kind: 'Service', group: '', port: 8080, weight: 3, futureField: true }, sibling]} onChange={onChange} />)
    fireEvent.mouseDown(screen.getAllByRole('combobox', { name: 'Backend kind' })[0])
    fireEvent.click(screen.getAllByText('EdgionBackend').find((element) => element.classList.contains('ant-select-item-option-content'))!)
    expect(onChange).toHaveBeenLastCalledWith([
      { name: 'primary', kind: 'EdgionBackend', group: 'edgion.io', weight: 3, futureField: true }, sibling,
    ])
  })

  it('retains zero backend weight instead of re-enabling traffic through the default weight', () => {
    const onChange = vi.fn()
    render(<BackendRefsEditor value={[{ name: 'provider', kind: 'EdgionBackend', group: 'edgion.io', weight: 1 }]} onChange={onChange} />)
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Backend weight' }), { target: { value: '0' } })
    expect(onChange).toHaveBeenLastCalledWith([{ name: 'provider', kind: 'EdgionBackend', group: 'edgion.io', weight: 0 }])
  })
})
