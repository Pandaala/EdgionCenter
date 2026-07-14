import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import LoginPage from './LoginPage'

const meMock = vi.fn()
vi.mock('../../api/auth', () => ({
  authApi: {
    me: () => meMock(),
    login: vi.fn(),
  },
}))

describe('LoginPage capabilities', () => {
  beforeEach(() => {
    sessionStorage.clear()
    meMock.mockReset()
    meMock.mockResolvedValue({ success: false })
  })

  it('renders password fields only when password login is supported', () => {
    render(<MemoryRouter><LoginPage passwordLogin /></MemoryRouter>)
    expect(screen.getByPlaceholderText('Username')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Password')).toBeInTheDocument()
  })

  it('uses external identity bootstrap when password login is unsupported', async () => {
    render(<MemoryRouter><LoginPage passwordLogin={false} /></MemoryRouter>)
    expect(await screen.findByText('External identity required')).toBeInTheDocument()
    expect(meMock).toHaveBeenCalledOnce()
    expect(screen.queryByPlaceholderText('Password')).not.toBeInTheDocument()
  })
})
