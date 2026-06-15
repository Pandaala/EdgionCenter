import { apiClient } from './client'
import type { ApiResponse, ListResponse } from './types'

/**
 * Serde view of a Center user (Full tier), mirroring the backend `UserDto`.
 * `password_hash` is never exposed. Field names are camelCase to match the
 * backend serde convention. Hits `center/admin/users[/{id}]`.
 */
export interface UserDto {
  id: number
  username: string
  displayName: string
  status: string
  createdAt: number
  roleIds: number[]
  roleNames: string[]
}

export interface CreateUserRequest {
  username: string
  password: string
  displayName?: string
  roleIds?: number[]
}

export interface UpdateUserRequest {
  status?: string
  password?: string
  roleIds?: number[]
}

export const usersApi = {
  list: async (): Promise<ListResponse<UserDto>> => {
    const { data } = await apiClient.get('center/admin/users')
    return data
  },
  create: async (req: CreateUserRequest): Promise<ApiResponse<number>> => {
    const { data } = await apiClient.post('center/admin/users', req)
    return data
  },
  update: async (id: number, req: UpdateUserRequest): Promise<ApiResponse<string>> => {
    const { data } = await apiClient.patch(`center/admin/users/${id}`, req)
    return data
  },
  remove: async (id: number): Promise<void> => {
    await apiClient.delete(`center/admin/users/${id}`)
  },
}
