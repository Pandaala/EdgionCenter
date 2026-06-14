import { describe, it, expect } from 'vitest'
import { isMenuItemVisible, type MenuGateContext } from './menuConfig'

const usersItem = { requiredPermission: 'users:manage', requiredUserMgmt: true as const }
const rolesItem = { requiredPermission: 'roles:manage', requiredAuthz: 'rbac' as const }
const auditItem = { requiredPermission: 'audit:read' }
const ungated = {}

/** Build a gate ctx, deriving `userMgmtAvailable` the same way Sidebar does. */
const ctx = (
  authzMode: 'allow_all' | 'rbac' | undefined,
  dbAuthEnabled: boolean,
  permissions: string[],
): MenuGateContext => ({
  authzMode,
  dbAuthEnabled,
  userMgmtAvailable: authzMode === 'rbac' || dbAuthEnabled,
  permissions,
})

describe('isMenuItemVisible', () => {
  it('hides Users and Roles when allow_all + dbAuth disabled, even with the keys', () => {
    const c = ctx('allow_all', false, ['users:manage', 'roles:manage'])
    expect(isMenuItemVisible(usersItem, c)).toBe(false)
    expect(isMenuItemVisible(rolesItem, c)).toBe(false)
  })

  it('shows Users but not Roles when the users table is in use (dbAuth) under allow_all', () => {
    const c = ctx('allow_all', true, ['users:manage', 'roles:manage'])
    expect(isMenuItemVisible(usersItem, c)).toBe(true)
    expect(isMenuItemVisible(rolesItem, c)).toBe(false)
  })

  it('shows both Users and Roles under rbac when the keys are held', () => {
    const c = ctx('rbac', false, ['users:manage', 'roles:manage'])
    expect(isMenuItemVisible(usersItem, c)).toBe(true)
    expect(isMenuItemVisible(rolesItem, c)).toBe(true)
  })

  it('hides Users when the permission key is missing even though the mode gate passes', () => {
    expect(isMenuItemVisible(usersItem, ctx('rbac', true, []))).toBe(false)
  })

  it('hides Roles when the permission key is missing even though authz is rbac', () => {
    expect(isMenuItemVisible(rolesItem, ctx('rbac', false, []))).toBe(false)
  })

  it('shows a permission-only item (audit) regardless of mode when granted', () => {
    expect(isMenuItemVisible(auditItem, ctx('allow_all', false, ['audit:read']))).toBe(true)
    expect(isMenuItemVisible(auditItem, ctx('rbac', false, ['audit:read']))).toBe(true)
  })

  it('hides a permission-only item when the permission is missing', () => {
    expect(isMenuItemVisible(auditItem, ctx('allow_all', false, []))).toBe(false)
  })

  it('always shows an item carrying neither gate', () => {
    expect(isMenuItemVisible(ungated, ctx(undefined, false, []))).toBe(true)
  })
})
