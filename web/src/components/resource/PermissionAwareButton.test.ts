import { describe, expect, it } from 'vitest'
import type { ControllerAccessDocument } from '@/api/types'
import { CONTROLLER_ACCESS_RESOURCE_KINDS, CONTROLLER_KIND_BY_RESOURCE_KIND } from '@/api/access'
import { getResourceCatalogEntry } from '@/config/resourceCatalog'
import { resolveActionAvailability, resolveControllerAccessId } from './PermissionAwareButton'

const access: ControllerAccessDocument = {
  schemaVersion: 1,
  revision: `sha256:${'a'.repeat(64)}`,
  resources: CONTROLLER_ACCESS_RESOURCE_KINDS.map((kind) => ({
    kind: CONTROLLER_KIND_BY_RESOURCE_KIND[kind],
    scope: getResourceCatalogEntry(kind).scope,
    verbs: kind === 'httproute' ? ['get', 'list', 'create', 'update'] : [],
  })),
  operations: [
    { name: 'regionRoute.list', allowed: true },
    { name: 'regionRoute.failover', allowed: false },
    { name: 'acme.trigger', allowed: false },
    { name: 'confSync.rotate', allowed: false },
    { name: 'reload', allowed: false },
    { name: 'serverInfo', allowed: true },
    { name: 'diagnostics', allowed: false },
    { name: 'wipeAll', allowed: false },
    { name: 'debug', allowed: false },
  ],
}

const base = {
  centerLoading: false,
  centerPermissions: ['proxy:read', 'proxy:write'],
  isControllerProxy: true,
}

describe('resolveActionAvailability', () => {
  it('uses the reactive route Controller before the layout-effect proxy target', () => {
    expect(resolveControllerAccessId('cluster-a~controller-1', null)).toBe('cluster-a/controller-1')
    expect(resolveControllerAccessId(undefined, 'direct-active')).toBe('direct-active')
  })

  it('denies an action while Center authorization is loading', () => {
    expect(resolveActionAvailability({ ...base, centerLoading: true })).toEqual({
      disabled: true,
      reason: 'Authorization is loading',
    })
  })

  it('requires a proxy permission for selected-Controller actions', () => {
    expect(resolveActionAvailability({
      ...base,
      centerPermissions: ['controllers:read'],
      controllerAccess: access,
      resourceKind: 'httproute',
      resourceVerb: 'update',
    })).toEqual({ disabled: true, reason: 'Missing permission: proxy:write' })
  })

  it('lets a read-only proxy grant view but not mutate', () => {
    const readOnly = { ...base, centerPermissions: ['proxy:read'], controllerAccess: access }
    expect(resolveActionAvailability({
      ...readOnly,
      resourceKind: 'httproute',
      resourceVerb: 'list',
    })).toEqual({ disabled: false })
    expect(resolveActionAvailability({
      ...readOnly,
      resourceKind: 'httproute',
      resourceVerb: 'update',
    })).toEqual({ disabled: true, reason: 'Missing permission: proxy:write' })
    // An operation carries no verb, so it needs the write half.
    expect(resolveActionAvailability({
      ...readOnly,
      operation: 'regionRoute.list',
    })).toEqual({ disabled: true, reason: 'Missing permission: proxy:write' })
  })

  it('denies reads to a write-only proxy grant', () => {
    expect(resolveActionAvailability({
      ...base,
      centerPermissions: ['proxy:write'],
      controllerAccess: access,
      resourceKind: 'httproute',
      resourceVerb: 'get',
    })).toEqual({ disabled: true, reason: 'Missing permission: proxy:read' })
  })

  it('does not require a Center proxy permission in direct mode', () => {
    expect(resolveActionAvailability({
      ...base,
      centerPermissions: [],
      isControllerProxy: false,
      controllerAccess: access,
      resourceKind: 'httproute',
      resourceVerb: 'update',
    })).toEqual({ disabled: false })
  })

  it('fails mutations closed when access is loading or unavailable', () => {
    expect(resolveActionAvailability({
      ...base,
      controllerAccessLoading: true,
      resourceKind: 'httproute',
      resourceVerb: 'create',
    }).disabled).toBe(true)
    expect(resolveActionAvailability({
      ...base,
      resourceKind: 'httproute',
      resourceVerb: 'create',
    })).toEqual({
      disabled: true,
      reason: 'Controller access is unavailable; mutations are disabled',
    })
  })

  it('intersects effective Controller resource and synthetic operation access', () => {
    expect(resolveActionAvailability({
      ...base,
      controllerAccess: access,
      resourceKind: 'httproute',
      resourceVerb: 'update',
    })).toEqual({ disabled: false })
    expect(resolveActionAvailability({
      ...base,
      controllerAccess: access,
      resourceKind: 'httproute',
      resourceVerb: 'delete',
    }).disabled).toBe(true)
    expect(resolveActionAvailability({
      ...base,
      controllerAccess: access,
      operation: 'regionRoute.failover',
    }).disabled).toBe(true)
  })

  it('still supports Center-only permission and capability gates', () => {
    expect(resolveActionAvailability({
      ...base,
      requiredPermission: 'roles:manage',
    }).disabled).toBe(true)
    expect(resolveActionAvailability({
      ...base,
      requiredCapability: 'roleAdmin',
      capabilities: { roleAdmin: true },
    })).toEqual({ disabled: false })
  })
})
