import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import { useEditorTabTransition } from './useEditorTabTransition'

function Harness({ onError }: { onError: (error: Error) => void }) {
  const [formData, setFormData] = useState({ name: 'initial' })
  const [yamlContent, setYamlContent] = useState('initial')
  const transition = useEditorTabTransition({
    formData,
    yamlContent,
    serialize: ({ name }) => name,
    parse: (source) => {
      if (source === 'invalid') throw new Error('invalid YAML')
      return { name: source }
    },
    setFormData,
    setYamlContent,
    onError,
  })
  return <>
    <output data-testid="active">{transition.activeTab}</output>
    <output data-testid="editable">{transition.editableTab}</output>
    <output data-testid="form-value">{formData.name}</output>
    <output data-testid="yaml-value">{yamlContent}</output>
    <button onClick={() => setFormData({ name: 'form-edit' })}>edit form</button>
    <button onClick={() => setYamlContent('yaml-edit')}>edit yaml</button>
    <button onClick={() => setYamlContent('invalid')}>break yaml</button>
    <button onClick={() => transition.handleTabChange('form')}>form</button>
    <button onClick={() => transition.handleTabChange('yaml')}>yaml</button>
    <button onClick={() => transition.handleTabChange('conditions')}>conditions</button>
  </>
}

describe('useEditorTabTransition', () => {
  it('keeps an unsaved Form draft intact across Conditions', () => {
    render(<Harness onError={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'edit form' }))
    fireEvent.click(screen.getByRole('button', { name: 'conditions' }))
    expect(screen.getByTestId('editable')).toHaveTextContent('form')
    fireEvent.click(screen.getByRole('button', { name: 'form' }))
    expect(screen.getByTestId('form-value')).toHaveTextContent('form-edit')
  })

  it('keeps an unsaved YAML draft intact across Conditions', () => {
    render(<Harness onError={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: 'yaml' }))
    fireEvent.click(screen.getByRole('button', { name: 'edit yaml' }))
    fireEvent.click(screen.getByRole('button', { name: 'conditions' }))
    fireEvent.click(screen.getByRole('button', { name: 'yaml' }))
    expect(screen.getByTestId('yaml-value')).toHaveTextContent('yaml-edit')
  })

  it('blocks an invalid YAML-to-Form transition without exposing stale data', () => {
    const onError = vi.fn()
    render(<Harness onError={onError} />)
    fireEvent.click(screen.getByRole('button', { name: 'yaml' }))
    fireEvent.click(screen.getByRole('button', { name: 'break yaml' }))
    fireEvent.click(screen.getByRole('button', { name: 'form' }))
    expect(screen.getByTestId('active')).toHaveTextContent('yaml')
    expect(screen.getByTestId('form-value')).toHaveTextContent('initial')
    expect(onError).toHaveBeenCalledWith(expect.objectContaining({ message: 'invalid YAML' }))
  })
})
