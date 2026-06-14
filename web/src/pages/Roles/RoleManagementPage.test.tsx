import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { RoleDto, PermissionGroup } from '@/api/roles'
import RoleManagementPage from './RoleManagementPage'

const rolesList = vi.fn()
const setPermissions = vi.fn()
const permissionCatalog = vi.fn()
vi.mock('@/api/roles', () => ({
  rolesApi: {
    list: () => rolesList(),
    create: vi.fn(),
    setPermissions: (id: number, keys: string[]) => setPermissions(id, keys),
    remove: vi.fn(),
    permissionCatalog: () => permissionCatalog(),
  },
}))

const ROLES: RoleDto[] = [
  { id: 1, name: 'ops', description: 'Operators', permissionKeys: ['audit:read'] },
]

const CATALOG: PermissionGroup[] = [
  { group: 'Audit', keys: ['audit:read'] },
  { group: 'Access Control', keys: ['users:manage', 'roles:manage'] },
]

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <RoleManagementPage />
    </QueryClientProvider>,
  )
}

describe('RoleManagementPage', () => {
  beforeEach(() => {
    rolesList.mockReset()
    setPermissions.mockReset()
    permissionCatalog.mockReset()
    rolesList.mockResolvedValue({ success: true, data: ROLES, count: ROLES.length })
    permissionCatalog.mockResolvedValue({ success: true, data: CATALOG })
    setPermissions.mockResolvedValue({ success: true, data: 'ok' })
  })

  it('renders the permission matrix from the mocked catalog', async () => {
    renderPage()
    // Keys from every catalog group render as matrix checkboxes.
    expect(await screen.findByRole('checkbox', { name: 'audit:read' })).toBeInTheDocument()
    expect(screen.getByRole('checkbox', { name: 'users:manage' })).toBeInTheDocument()
    expect(screen.getByRole('checkbox', { name: 'roles:manage' })).toBeInTheDocument()
    // The auto-selected role's existing permission is pre-checked.
    expect(screen.getByRole('checkbox', { name: 'audit:read' })).toBeChecked()
  })

  it('saves toggled permissions via setPermissions', async () => {
    renderPage()
    const usersBox = await screen.findByRole('checkbox', { name: 'users:manage' })
    fireEvent.click(usersBox)

    fireEvent.click(screen.getByRole('button', { name: 'Save Permissions' }))

    await waitFor(() => {
      expect(setPermissions).toHaveBeenCalledWith(1, ['audit:read', 'users:manage'])
    })
  })
})
