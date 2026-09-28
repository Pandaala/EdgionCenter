import { useState } from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { Link, MemoryRouter, Route, Routes } from 'react-router-dom'
import { expect, it, vi } from 'vitest'
import ControllerProxy from './ControllerProxy'

vi.mock('../shell/AppShell', () => ({ AppShell: () => {
  const [selected, setSelected] = useState(false)
  return <><button onClick={() => setSelected(true)}>Select resource</button>{selected && <div>Selected resource and editor draft</div>}<Link to="/controller/west~controller">Switch Controller</Link></>
} }))

it('discards Controller-local selections and drafts when the route Controller changes', () => {
  render(<MemoryRouter initialEntries={['/controller/east~controller']}><Routes><Route path="/controller/:controllerId" element={<ControllerProxy />} /></Routes></MemoryRouter>)
  fireEvent.click(screen.getByRole('button', { name: 'Select resource' }))
  expect(screen.getByText('Selected resource and editor draft')).toBeInTheDocument()
  fireEvent.click(screen.getByRole('link', { name: 'Switch Controller' }))
  expect(screen.queryByText('Selected resource and editor draft')).not.toBeInTheDocument()
})
