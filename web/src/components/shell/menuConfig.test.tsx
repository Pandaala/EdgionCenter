import { describe, it, expect } from 'vitest'
import { isMenuItemVisible, type MenuGateContext } from './menuConfig'

const usersItem = { requiredPermission: 'users:manage', requiredCapability: 'userAdmin' as const }
const rolesItem = { requiredPermission: 'roles:manage', requiredCapability: 'roleAdmin' as const }
const auditItem = { requiredPermission: 'audit:read', requiredCapability: 'auditQuery' as const }
const historyItem = { requiredPermission: 'controllers:read', requiredCapability: 'controllerHistory' as const }
const ungated = {}

const ctx = (
  permissions: string[],
  capabilities: MenuGateContext['capabilities'] = {},
): MenuGateContext => ({
  capabilities,
  permissions,
})

describe('isMenuItemVisible', () => {
  it('hides SQL management when capabilities are unavailable, even with permission keys', () => {
    const c = ctx(['users:manage', 'roles:manage'])
    expect(isMenuItemVisible(usersItem, c)).toBe(false)
    expect(isMenuItemVisible(rolesItem, c)).toBe(false)
  })

  it('resolves user and role management independently from backend capabilities', () => {
    const c = ctx(['users:manage', 'roles:manage'], { userAdmin: true })
    expect(isMenuItemVisible(usersItem, c)).toBe(true)
    expect(isMenuItemVisible(rolesItem, c)).toBe(false)
  })

  it('shows both Users and Roles when capabilities and keys are present', () => {
    const c = ctx(['users:manage', 'roles:manage'], { userAdmin: true, roleAdmin: true })
    expect(isMenuItemVisible(usersItem, c)).toBe(true)
    expect(isMenuItemVisible(rolesItem, c)).toBe(true)
  })

  it('hides Users when the permission key is missing even though the mode gate passes', () => {
    expect(isMenuItemVisible(usersItem, ctx([], { userAdmin: true }))).toBe(false)
  })

  it('hides Roles when the permission key is missing even though authz is rbac', () => {
    expect(isMenuItemVisible(rolesItem, ctx([], { roleAdmin: true }))).toBe(false)
  })

  it('shows a permission-only item (audit) regardless of mode when granted', () => {
    expect(isMenuItemVisible(auditItem, ctx(['audit:read'], { auditQuery: true }))).toBe(true)
  })

  it('hides a permission-only item when the permission is missing', () => {
    expect(isMenuItemVisible(auditItem, ctx([], { auditQuery: true }))).toBe(false)
  })

  it('shows controller history only when its capability is resolved', () => {
    expect(isMenuItemVisible(historyItem, ctx([]))).toBe(false)
    expect(isMenuItemVisible(historyItem, ctx([], { controllerHistory: true }))).toBe(false)
    expect(isMenuItemVisible(historyItem, ctx(['controllers:read'], { controllerHistory: true }))).toBe(true)
  })

  it('always shows an item carrying neither gate', () => {
    expect(isMenuItemVisible(ungated, ctx([]))).toBe(true)
  })
})
