import { describe, it, expect } from 'vitest'
import { isMenuItemVisible } from './menuConfig'

const usersItem = { requiredPermission: 'users:manage', requiredMode: 'full' as const }
const auditItem = { requiredPermission: 'audit:read' }
const ungated = {}

describe('isMenuItemVisible', () => {
  it('hides full-mode items in lite even when the permission is granted', () => {
    // Lite grants users:manage via AllowAllAuthz, but the item is full-only.
    expect(isMenuItemVisible(usersItem, { accessMode: 'lite', permissions: ['users:manage'] })).toBe(false)
  })

  it('shows full-mode items in full when the permission is held', () => {
    expect(isMenuItemVisible(usersItem, { accessMode: 'full', permissions: ['users:manage'] })).toBe(true)
  })

  it('hides full-mode items when the permission is missing', () => {
    expect(isMenuItemVisible(usersItem, { accessMode: 'full', permissions: [] })).toBe(false)
  })

  it('shows a permission-only item (audit) in both modes when granted', () => {
    expect(isMenuItemVisible(auditItem, { accessMode: 'lite', permissions: ['audit:read'] })).toBe(true)
    expect(isMenuItemVisible(auditItem, { accessMode: 'full', permissions: ['audit:read'] })).toBe(true)
  })

  it('hides a permission-only item when the permission is missing', () => {
    expect(isMenuItemVisible(auditItem, { accessMode: 'lite', permissions: [] })).toBe(false)
  })

  it('always shows an item carrying neither gate', () => {
    expect(isMenuItemVisible(ungated, { accessMode: undefined, permissions: [] })).toBe(true)
  })
})
