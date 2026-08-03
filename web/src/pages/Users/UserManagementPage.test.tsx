import { describe, it, expect, vi, beforeEach } from 'vitest'
import { screen, fireEvent, waitFor, within } from '@testing-library/react'
import type { UserDto } from '@/api/users'
import { renderWithQueryClient } from '@/test/render'
import UserManagementPage from './UserManagementPage'

const usersList = vi.fn()
const usersCreate = vi.fn()
vi.mock('@/api/users', () => ({
  usersApi: {
    list: () => usersList(),
    create: (req: unknown) => usersCreate(req),
    update: vi.fn(),
    remove: vi.fn(),
  },
}))

const rolesList = vi.fn()
vi.mock('@/api/roles', () => ({
  rolesApi: {
    list: () => rolesList(),
  },
}))

const USERS: UserDto[] = [
  {
    id: 1,
    username: 'alice',
    displayName: 'Alice',
    status: 'active',
    createdAt: 1_700_000_000,
    roleIds: [1],
    roleNames: ['admin'],
  },
  {
    id: 2,
    username: 'bob',
    displayName: 'Bob',
    status: 'disabled',
    createdAt: 1_700_000_100,
    roleIds: [],
    roleNames: [],
  },
]

function renderPage() {
  return renderWithQueryClient(<UserManagementPage />)
}

describe('UserManagementPage', () => {
  beforeEach(() => {
    usersList.mockReset()
    usersCreate.mockReset()
    rolesList.mockReset()
    usersList.mockResolvedValue({ success: true, data: USERS, count: USERS.length })
    usersCreate.mockResolvedValue({ success: true, data: 3 })
    rolesList.mockResolvedValue({ success: true, data: [{ id: 1, name: 'admin', description: '', permissionKeys: [] }], count: 1 })
  })

  it('renders user rows from the mocked API', async () => {
    renderPage()
    expect(await screen.findByText('alice')).toBeInTheDocument()
    expect(screen.getByText('bob')).toBeInTheDocument()
  })

  it('creates a user from the Create modal', async () => {
    renderPage()
    await screen.findByText('alice')

    fireEvent.click(screen.getByRole('button', { name: 'Create' }))

    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Username'), { target: { value: 'carol' } })
    fireEvent.change(within(dialog).getByLabelText('Password'), { target: { value: 'pw-carol-123' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }))

    await waitFor(() => {
      expect(usersCreate).toHaveBeenCalledWith(
        expect.objectContaining({ username: 'carol', password: 'pw-carol-123' }),
      )
    })
  })
})
