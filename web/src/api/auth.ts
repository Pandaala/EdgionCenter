import { apiClient } from './client'
import type { ApiResponse } from './types'

export interface LoginRequest {
  username: string
  password: string
}

export interface LoginResponse {
  token: string
  expires_in: number
}

export interface MeResponse {
  username: string
}

export const authApi = {
  login: async (req: LoginRequest): Promise<ApiResponse<LoginResponse>> => {
    const { data } = await apiClient.post('auth/login', req, { _silent: true } as any)
    return data
  },
  logout: async (): Promise<void> => {
    await apiClient.post('auth/logout', null, { _silent: true } as any).catch(() => {})
  },
  me: async (): Promise<ApiResponse<MeResponse>> => {
    const { data } = await apiClient.get('auth/me', { _silent: true } as any)
    return data
  },
}
