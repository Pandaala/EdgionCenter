import { beforeEach, describe, expect, it, vi } from 'vitest'

const { getMock, postMock } = vi.hoisted(() => ({
  getMock: vi.fn(),
  postMock: vi.fn(),
}))

vi.mock('./client', () => ({
  apiClient: {
    get: getMock,
    post: postMock,
  },
}))

import { authApi } from './auth'

describe('Center authentication API routing', () => {
  beforeEach(() => {
    getMock.mockReset()
    postMock.mockReset()
  })

  it('keeps the current-user request on Center when a Controller proxy is active', async () => {
    getMock.mockResolvedValue({ data: { success: true, data: { username: 'admin' } } })

    await authApi.me()

    expect(getMock).toHaveBeenCalledWith('auth/me', expect.objectContaining({
      _silent: true,
      _skipControllerProxy: true,
    }))
  })

  it('keeps login and logout on Center', async () => {
    postMock
      .mockResolvedValueOnce({ data: { success: true, data: { token: 'token', expires_in: 60 } } })
      .mockResolvedValueOnce({ data: { success: true } })

    await authApi.login({ username: 'admin', password: 'secret' })
    await authApi.logout()

    expect(postMock).toHaveBeenNthCalledWith(1, 'auth/login', {
      username: 'admin',
      password: 'secret',
    }, expect.objectContaining({ _skipControllerProxy: true }))
    expect(postMock).toHaveBeenNthCalledWith(2, 'auth/logout', null, expect.objectContaining({
      _skipControllerProxy: true,
    }))
  })
})
