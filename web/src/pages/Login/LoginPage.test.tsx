import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
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
    localStorage.clear()
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

  it('returns an externally authenticated session to its requested deep link', async () => {
    meMock.mockResolvedValue({ success: true, data: { username: 'oidc-user' } })
    render(
      <MemoryRouter initialEntries={[{ pathname: '/login', state: { from: '/controller/e2e-a~controller-a/topology' } }]}>
        <Routes>
          <Route path="/login" element={<LoginPage passwordLogin={false} />} />
          <Route path="/controller/:controllerId/topology" element={<div>Controller topology</div>} />
        </Routes>
      </MemoryRouter>,
    )
    expect(await screen.findByText('Controller topology')).toBeInTheDocument()
    expect(localStorage.getItem('edgion-logged-in')).toBe('1')
  })

  it('restores the deep link saved by a stale-session 401', async () => {
    sessionStorage.setItem('edgion-login-return-path', '/controller/e2e-b~controller-b/resources?kind=Secret#selected')
    meMock.mockResolvedValue({ success: true, data: { username: 'oidc-user' } })
    render(
      <MemoryRouter initialEntries={['/login']}>
        <Routes>
          <Route path="/login" element={<LoginPage passwordLogin={false} />} />
          <Route path="/controller/:controllerId/resources" element={<div>Controller resources</div>} />
        </Routes>
      </MemoryRouter>,
    )
    expect(await screen.findByText('Controller resources')).toBeInTheDocument()
    expect(sessionStorage.getItem('edgion-login-return-path')).toBeNull()
  })
})
