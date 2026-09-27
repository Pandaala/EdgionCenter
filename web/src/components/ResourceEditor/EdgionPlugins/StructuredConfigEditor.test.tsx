import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import StructuredConfigEditor from './StructuredConfigEditor'

const fields = [{ name: 'hideCredentials', kind: 'boolean' as const }]

describe('optional plugin booleans', () => {
  it('distinguishes omission from explicit false without materializing defaults', () => {
    const onChange = vi.fn()
    const view = render(<StructuredConfigEditor fields={fields} value={{ futureField: 'keep' }} onChange={onChange} readOnly={false} />)
    expect(screen.getByText('—')).toBeInTheDocument()
    expect(onChange).not.toHaveBeenCalled()
    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'hideCredentials' }))
    fireEvent.click(screen.getByText('false', { selector: '.ant-select-item-option-content' }))
    expect(onChange).toHaveBeenLastCalledWith({ futureField: 'keep', hideCredentials: false })
    view.rerender(<StructuredConfigEditor fields={fields} value={{ hideCredentials: false }} onChange={onChange} readOnly={false} />)
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    expect(onChange).toHaveBeenLastCalledWith({})
  })
})
