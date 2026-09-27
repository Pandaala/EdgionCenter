import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { TopBar } from './TopBar'
import { authApi } from '@/api/auth'
import { setAppMode } from '@/utils/proxy'

vi.mock('@/api/auth', () => ({ authApi: { me: vi.fn(), logout: vi.fn() } }))
vi.mock('@/components/widgets/ThemeToggle', () => ({ ThemeToggle: () => null }))

async function requestLogout() {
  render(<MemoryRouter initialEntries={['/controllers']}><Routes>
    <Route path="/controllers" element={<TopBar collapsed={false} onToggleCollapse={() => {}} />} />
    <Route path="/login" element={<div>Password login page</div>} />
  </Routes></MemoryRouter>)
  const user = userEvent.setup()
  await user.hover(screen.getByTestId('user-menu'))
  await user.click(await screen.findByTestId('logout'))
}

describe('Session logout', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    setAppMode('center')
    localStorage.setItem('edgion-logged-in', '1')
    vi.mocked(authApi.me).mockResolvedValue({ success: true, data: { username: 'user', authProvider: 'local' } })
    vi.mocked(authApi.logout).mockResolvedValue(undefined)
  })

  it('clears local session state only after password logout succeeds', async () => {
    await requestLogout()
    expect(await screen.findByText('Password login page')).toBeInTheDocument()
    expect(authApi.logout).toHaveBeenCalledOnce()
    expect(localStorage.getItem('edgion-logged-in')).toBeNull()
  })

  it('keeps session state and reports a failed logout', async () => {
    vi.mocked(authApi.logout).mockRejectedValueOnce(new Error('private transport detail'))
    await requestLogout()
    expect(await screen.findByText(/Could not complete logout/)).toBeInTheDocument()
    expect(localStorage.getItem('edgion-logged-in')).toBe('1')
    expect(screen.queryByText('Password login page')).not.toBeInTheDocument()
    expect(screen.queryByText(/private transport detail/)).not.toBeInTheDocument()
  })

  it('does not call password logout for an OIDC session without a sign-out link', async () => {
    vi.mocked(authApi.me).mockResolvedValueOnce({ success: true, data: { username: 'external', authProvider: 'oidc' } })
    await requestLogout()
    expect(await screen.findByText(/Logout is managed by your sign-in provider/)).toBeInTheDocument()
    expect(authApi.logout).not.toHaveBeenCalled()
    expect(localStorage.getItem('edgion-logged-in')).toBe('1')
  })

  it.each(['//other.example/logout', '/\\other.example/logout', '/logout\n'])('rejects unsafe external logout path %j', async logoutPath => {
    vi.mocked(authApi.me).mockResolvedValueOnce({ success: true, data: { username: 'external', authProvider: 'oidc', logoutPath } })
    await requestLogout()
    await waitFor(() => expect(screen.getAllByText(/Could not complete logout/).length).toBeGreaterThan(0))
    expect(authApi.logout).not.toHaveBeenCalled()
    expect(localStorage.getItem('edgion-logged-in')).toBe('1')
  })

  it('does not guess a provider when the identity request fails', async () => {
    vi.mocked(authApi.me).mockRejectedValueOnce(new Error('private identity error'))
    await requestLogout()
    await waitFor(() => expect(screen.getAllByText(/Could not complete logout/).length).toBeGreaterThan(0))
    expect(authApi.logout).not.toHaveBeenCalled()
    expect(localStorage.getItem('edgion-logged-in')).toBe('1')
  })

  it('stops external logout when clearing a coexisting password cookie fails', async () => {
    vi.mocked(authApi.me).mockResolvedValueOnce({ success: true, data: {
      username: 'external', authProvider: 'oidc', logoutPath: '/oauth2/sign_out', localLogoutAvailable: true,
    } })
    vi.mocked(authApi.logout).mockRejectedValueOnce(new Error('transport failure'))
    await requestLogout()
    await waitFor(() => expect(screen.getAllByText(/Could not complete logout/).length).toBeGreaterThan(0))
    expect(authApi.logout).toHaveBeenCalledOnce()
    expect(localStorage.getItem('edgion-logged-in')).toBe('1')
  })
})
