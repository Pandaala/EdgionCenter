import { apiClient } from './client'
import type { ApiResponse, ListResponse } from './types'

/**
 * Serde view of a Center role (Full tier) plus its permission keys, mirroring
 * the backend `RoleDto`. Hits `center/admin/roles[...]`.
 */
export interface RoleDto {
  id: number
  name: string
  description: string
  permissionKeys: string[]
}

/** One group of the permission catalog for the role/permission matrix UI. */
export interface PermissionGroup {
  group: string
  keys: string[]
}

export interface CreateRoleRequest {
  name: string
  description?: string
  permissionKeys?: string[]
}

export const rolesApi = {
  list: async (): Promise<ListResponse<RoleDto>> => {
    const { data } = await apiClient.get('center/admin/roles')
    return data
  },
  create: async (req: CreateRoleRequest): Promise<ApiResponse<number>> => {
    const { data } = await apiClient.post('center/admin/roles', req)
    return data
  },
  setPermissions: async (id: number, keys: string[]): Promise<ApiResponse<string>> => {
    const { data } = await apiClient.put(`center/admin/roles/${id}/permissions`, {
      permissionKeys: keys,
    })
    return data
  },
  remove: async (id: number): Promise<void> => {
    await apiClient.delete(`center/admin/roles/${id}`)
  },
  permissionCatalog: async (): Promise<ApiResponse<PermissionGroup[]>> => {
    const { data } = await apiClient.get('center/admin/permission-catalog')
    return data
  },
}
