import { describe, expect, it } from 'vitest'
import {
  centerMenu,
  filterMenuTree,
  isMenuItemVisible,
  isMenuNodeActive,
  type MenuBranch,
  type MenuGateContext,
  type MenuLeaf,
  type MenuNode,
} from './menuConfig'

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

const findCenterSection = (labelKey: string) => {
  const section = centerMenu.find((item) => item.labelKey === labelKey)
  if (!section) throw new Error(`Center menu section is missing: ${labelKey}`)
  return section
}

const findBranch = (nodes: MenuNode[], key: string): MenuBranch => {
  for (const node of nodes) {
    if (node.kind === 'group') {
      if (node.key === key) return node
      try {
        return findBranch(node.children, key)
      } catch {
        // Search the next branch.
      }
    }
  }
  throw new Error(`Menu branch is missing: ${key}`)
}

const flattenLeaves = (nodes: MenuNode[]): MenuLeaf[] => nodes.flatMap((node) =>
  node.kind === 'item' ? [node] : flattenLeaves(node.children),
)

describe('menu access gates', () => {
  it('requires both capabilities and permission keys', () => {
    expect(isMenuItemVisible(usersItem, ctx(['users:manage']))).toBe(false)
    expect(isMenuItemVisible(rolesItem, ctx(['roles:manage'], { userAdmin: true }))).toBe(false)
    expect(isMenuItemVisible(usersItem, ctx([], { userAdmin: true }))).toBe(false)
    expect(isMenuItemVisible(usersItem, ctx(['users:manage'], { userAdmin: true }))).toBe(true)
    expect(isMenuItemVisible(rolesItem, ctx(['roles:manage'], { roleAdmin: true }))).toBe(true)
  })

  it('supports permission-only, capability-gated, and ungated entries', () => {
    expect(isMenuItemVisible(auditItem, ctx([], { auditQuery: true }))).toBe(false)
    expect(isMenuItemVisible(auditItem, ctx(['audit:read'], { auditQuery: true }))).toBe(true)
    expect(isMenuItemVisible(historyItem, ctx(['controllers:read']))).toBe(false)
    expect(isMenuItemVisible(historyItem, ctx(['controllers:read'], { controllerHistory: true }))).toBe(true)
    expect(isMenuItemVisible(ungated, ctx([]))).toBe(true)
  })

  it('recursively filters arbitrary-depth trees and removes empty ancestors', () => {
    const tree: MenuNode[] = [{
      kind: 'group',
      key: 'root',
      labelKey: 'Root',
      children: [
        {
          kind: 'group',
          key: 'empty-parent',
          labelKey: 'Empty',
          children: [{
            kind: 'item',
            key: 'denied',
            labelKey: 'Denied',
            path: '/denied',
            requiredPermission: 'denied:read',
          }],
        },
        {
          kind: 'group',
          key: 'visible-parent',
          labelKey: 'Visible',
          children: [{
            kind: 'group',
            key: 'deep-parent',
            labelKey: 'Deep',
            children: [{ kind: 'item', key: 'visible', labelKey: 'Visible', path: '/visible' }],
          }],
        },
      ],
    }]

    const filtered = filterMenuTree(tree, ctx([]))
    expect(filtered).toHaveLength(1)
    expect(findBranch(filtered, 'root').children.map((node) => node.key)).toEqual(['visible-parent'])
    expect(flattenLeaves(filtered).map((leaf) => leaf.key)).toEqual(['visible'])
  })

  it('propagates descendant active state through every ancestor', () => {
    const tree: MenuNode = {
      kind: 'group',
      key: 'root',
      labelKey: 'Root',
      children: [{
        kind: 'group',
        key: 'nested',
        labelKey: 'Nested',
        children: [{ kind: 'item', key: 'target', labelKey: 'Target', path: '/target' }],
      }],
    }

    expect(isMenuNodeActive(tree, (path) => path === '/target')).toBe(true)
    expect(isMenuNodeActive(findBranch(tree.children, 'nested'), (path) => path === '/target')).toBe(true)
    expect(isMenuNodeActive(tree, (path) => path === '/other')).toBe(false)
  })
})

describe('Center navigation structure', () => {
  it('uses the four requested navigation sections', () => {
    expect(centerMenu.map((section) => section.labelKey)).toEqual([
      'center.nav.section.federation',
      'center.nav.section.traffic',
      'center.nav.section.cloud',
      'center.nav.section.system',
    ])
  })

  it('groups Controller operations under Federation and account administration under System Management', () => {
    expect(findCenterSection('center.nav.section.federation').children.map((item) => item.key)).toEqual([
      'center-dashboard',
      'center-controllers',
    ])
    expect(findCenterSection('center.nav.section.system').children.map((item) => item.key)).toEqual([
      'center-audit',
      'center-users',
      'center-roles',
    ])
  })

  // RegionRoute has exactly one override dimension, so it is a single leaf
  // rather than a group. The removed Service dimension had no backing type in
  // Edgion; per-service failover is expressed by pointing one EdgionPlugins
  // entry's `overrideRef` at its own RegionRouteOverride.
  it('exposes RegionRoute as a single leaf under Traffic', () => {
    const leaves = flattenLeaves(findCenterSection('center.nav.section.traffic').children)
    const regionRoutes = leaves.filter((item) => item.path?.startsWith('/region-routes'))
    expect(regionRoutes.map((item) => item.path)).toEqual(['/region-routes/region'])
    expect(regionRoutes.every((item) => item.requiredPermission === 'region-routes:read')).toBe(true)
  })

  it('defines the GlobalResources inventory menu behind both gates', () => {
    const globalResources = findBranch(
      findCenterSection('center.nav.section.traffic').children,
      'center-global-resources',
    )
    expect(globalResources.requiredPermission).toBe('global-resources:read')
    expect(globalResources.requiredCapability).toBe('globalResourcesInventory')
    expect(globalResources.children.map((node) => node.key)).toEqual([
      'center-global-config-data-ip-list',
      'center-global-config-data-key-list',
      'center-global-config-data-selector',
      'center-global-config-data-misc',
    ])
    expect(flattenLeaves(globalResources.children).map((item) => item.path)).toEqual([
      '/global-resources/edgion-config-data/ip-list',
      '/global-resources/edgion-config-data/key-list',
      '/global-resources/edgion-config-data/selector',
      '/global-resources/edgion-config-data/misc',
    ])

    expect(filterMenuTree([globalResources], ctx(['global-resources:read']))).toEqual([])
    expect(filterMenuTree(
      [globalResources],
      ctx([], { globalResourcesInventory: true }),
    )).toEqual([])
    expect(filterMenuTree(
      [globalResources],
      ctx(['global-resources:read'], { globalResourcesInventory: true }),
    )).toHaveLength(1)
  })

  it('removes temporary Global rule entries and keeps legacy IP Lists outside GlobalResources', () => {
    const leaves = centerMenu.flatMap((section) => flattenLeaves(section.children))
    expect(leaves.map((item) => item.key)).not.toContain('center-global-shared-plugins')
    expect(leaves.map((item) => item.key)).not.toContain('center-global-ip-lists')
    expect(leaves.map((item) => item.path)).not.toContain('/global-rules/ip-lists')
  })

  it('keeps the existing Cloud Services permission composition', () => {
    const cloudflare = findBranch(
      findCenterSection('center.nav.section.cloud').children,
      'center-cloudflare',
    )
    const dns = flattenLeaves(cloudflare.children).find((item) => item.key === 'center-cloudflare-dns')
    if (!dns) throw new Error('Cloudflare DNS item is missing')
    expect(isMenuItemVisible(
      dns,
      ctx(['cloudflare-dns:read'], { cloudflareDnsRead: true }),
    )).toBe(false)
    expect(isMenuItemVisible(
      dns,
      ctx(['cloudflare-dns:read', 'provider-accounts:read'], { cloudflareDnsRead: true }),
    )).toBe(true)
  })
})
