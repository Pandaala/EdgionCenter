import { expect, test, type APIRequestContext, type Locator, type Page } from '@playwright/test'
import { createHash } from 'node:crypto'
import { controllerId, controllerPathId } from '../support/controllers.ts'

const controllerPage = (path = '') => `/controller/${controllerPathId('A')}${path}`
const runId = process.env.E2E_RUN_ID ?? 'resource-ui-local'
const prefix = `eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}`
const namespace = `${prefix}-a`

async function configData(request: APIRequestContext, slot: 'A' | 'B', name: string) {
  const response = await request.get(`/api/v1/proxy/${controllerPathId(slot)}/api/v1/namespaced/edgionconfigdata/${namespace}/${name}`)
  expect(response.ok()).toBeTruthy()
  return response.json() as Promise<{ metadata?: { labels?: Record<string, string> }; spec?: { data?: { config?: { active?: string; description?: string; regions?: Array<{ name?: string; failoverTo?: string }> } } } }>
}

async function restoreSelector(request: APIRequestContext, pluginName: string) {
  const response = await request.patch(
    `/api/v1/center/global-connection-ip-restrictions/${namespace}/${pluginName}/active-profile`,
    { data: { activeProfile: 'open', controllers: [controllerId('A')] } },
  )
  expect(response.ok()).toBeTruthy()
}

async function setRegionFailover(request: APIRequestContext, failoverTo: string) {
  const response = await request.post('/api/v1/center/region-routes/failover', {
    data: {
      namespace,
      name: '',
      pluginName: `${prefix}-region-route`,
      entryIndex: 0,
      regionName: 'east',
      failoverTo,
    },
  })
  expect(response.status()).toBe(200)
  const body = await response.json() as { success?: boolean; data?: { modified?: number; failed?: number } }
  expect(body).toMatchObject({ success: true, data: { modified: 2, failed: 0 } })
  return response
}

async function clickAndWaitForGet(page: Page, testId: string, path: string) {
  await Promise.all([
    page.waitForResponse((response) => response.request().method() === 'GET' && response.url().includes(path)),
    page.getByTestId(testId).click(),
  ])
}

async function cancelModal(page: Page, cancelTestId: string) {
  const cancel = page.getByTestId(cancelTestId)
  await expect(cancel).toBeVisible()
  await cancel.click()
  await expect(cancel).toBeHidden()
}

async function openFirstAvailable(locator: Locator): Promise<boolean> {
  if (await locator.count() === 0) return false
  await locator.first().click()
  return true
}

type CenterCapability = 'auditQuery' | 'controllerHistory' | 'roleAdmin' | 'userAdmin'

async function hasCapability(request: APIRequestContext, capability: CenterCapability): Promise<boolean> {
  const response = await request.get('/api/v1/server-info')
  if (!response.ok()) return false
  const body = await response.json() as { data?: { capabilities?: Partial<Record<CenterCapability, boolean>> } }
  return body.data?.capabilities?.[capability] === true
}

async function applyTableFilter(page: Page, titleTestId: string, value: string) {
  const header = page.getByTestId(titleTestId).locator('xpath=ancestor::th')
  await header.locator('.ant-table-filter-trigger').click()
  const dropdown = page.locator('.ant-dropdown:visible')
  await dropdown.locator('.ant-dropdown-menu-item').filter({ hasText: value }).click()
  await dropdown.locator('.ant-table-filter-dropdown-btns .ant-btn-primary').click()
}

test.describe('Center and shell actions', () => {
  test('shell toggles navigation and language, reloads, and logs out', async ({ page }) => {
    await page.goto('/')

    await page.getByTestId('nav-toggle').click()
    const language = page.getByTestId('language-toggle')
    const before = (await language.textContent())?.trim()
    await language.click()
    await expect(language).not.toHaveText(before ?? '')

    await page.getByTestId('user-menu').click()
    await expect(page.getByTestId('logout')).toBeVisible()
    await page.keyboard.press('Escape')

    await Promise.all([
      page.waitForEvent('load'),
      page.getByTestId('page-reload').click(),
    ])
    await expect(page.getByTestId('nav-toggle')).toBeVisible()

    await page.getByTestId('user-menu').click()
    await Promise.all([
      page.waitForURL(/\/login$/),
      page.getByTestId('logout').click(),
    ])
  })

  test('Center dashboard refreshes, filters, reloads, and enters a controller', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByTestId('controller-card-enter').first()).toBeVisible()

    await clickAndWaitForGet(page, 'controllers-refresh', '/api/v1/controllers')

    const cards = page.getByTestId('controller-card-enter')
    const cardCount = await cards.count()
    const search = page.getByTestId('controller-search')
    await search.fill('__no_controller_matches__')
    await search.press('Enter')
    await expect(page.getByTestId('controller-card-enter')).toHaveCount(0)
    await search.fill(controllerId('A'))
    await search.press('Enter')
    await expect(page.getByTestId('controller-card-enter').first()).toBeVisible()
    await search.fill('')
    await expect(page.getByTestId('controller-card-enter')).toHaveCount(cardCount)

    const clusterFilter = page.getByTestId('controller-cluster-filter')
    await clusterFilter.click()
    const clusterOption = page.locator('.ant-select-dropdown:visible .ant-select-item-option').nth(1)
    if (await clusterOption.count()) {
      await clusterOption.click()
      await expect(page.getByTestId('controller-card-enter').first()).toBeVisible()
    } else {
      await page.keyboard.press('Escape')
    }

    await page.getByTestId('controller-card-reload').first().click()
    await cancelModal(page, 'controller-card-reload-cancel')
    await page.getByTestId('controller-card-reload').first().click()
    await Promise.all([
      page.waitForResponse((response) => response.request().method() === 'POST' && /\/controllers\/[^/]+\/reload$/.test(response.url())),
      page.getByTestId('controller-card-reload-confirm').click(),
    ])

    await page.getByTestId('controller-card-enter').first().click()
    await expect(page).toHaveURL(/\/controller\//)
  })

  test('controller dashboards and topology execute refresh, filter, legend, and node actions', async ({ page }) => {
    await page.goto(controllerPage())
    await clickAndWaitForGet(page, 'dashboard-refresh', '/api/v1/')

    await page.goto(controllerPage('/user'))
    await clickAndWaitForGet(page, 'user-refresh', '/api/v1/')

    await page.goto(controllerPage('/topology'))
    await expect(page.getByTestId('topology-canvas')).toBeVisible()
    await page.getByTestId('topology-refresh').click()
    await page.getByTestId('topology-legend').click()
    await expect(page.locator('.ant-popover')).toBeVisible()
    await page.keyboard.press('Escape')

    const namespaceFilter = page.getByTestId('topology-namespace-filter')
    await namespaceFilter.click()
    const option = page.locator('.ant-select-dropdown:visible .ant-select-item-option').first()
    if (await option.count()) await option.click()

    const node = page.getByTestId('topology-node').first()
    if (await node.count()) {
      await node.click()
      await expect(page.getByRole('dialog')).toBeVisible()
      await page.keyboard.press('Escape')
    }
  })

  test('audit controls apply/reset filters, refresh, and paginate', async ({ page, request }) => {
    test.setTimeout(60_000)
    const canSeedAudit = await hasCapability(request, 'auditQuery') && await hasCapability(request, 'userAdmin')
    test.skip(!canSeedAudit, `${process.env.E2E_MODE} runtime cannot query and seed audit events`)
    const suffix = (process.env.E2E_RUN_ID ?? `${Date.now()}`).replace(/[^A-Za-z0-9-]/g, '').slice(-24)
    const username = `e2e-audit-${suffix}`
    const existingUsers = await request.get('/api/v1/center/admin/users')
    if (existingUsers.ok()) {
      const body = await existingUsers.json() as { data?: Array<{ id: number; username: string }> }
      const existing = body.data?.find((item) => item.username === username)
      if (existing) await request.delete(`/api/v1/center/admin/users/${existing.id}`)
    }
    const created = await request.post('/api/v1/center/admin/users', {
      data: { username, password: 'E2e-Audit-Password-2026!', displayName: 'Playwright audit fixture', roleIds: [] },
    })
    expect(created.ok()).toBeTruthy()
    const createdBody = await created.json() as { data?: number }
    const userId = createdBody.data
    if (typeof userId !== 'number') throw new Error('Audit fixture user id is unavailable')
    try {
      for (let offset = 0; offset < 55; offset += 5) {
        const responses = await Promise.all(Array.from({ length: Math.min(5, 55 - offset) }, (_, index) => (
          request.patch(`/api/v1/center/admin/users/${userId}`, {
            data: { status: (offset + index) % 2 === 0 ? 'disabled' : 'active' },
          })
        )))
        expect(responses.every((response) => response.ok())).toBeTruthy()
      }
      await expect.poll(async () => {
        const response = await request.get('/api/v1/center/admin/audit-logs?limit=50&offset=0')
        const body = await response.json() as { data?: unknown[] }
        return body.data?.length ?? 0
      }).toBe(50)

      await page.goto('/audit')
      await expect(page.getByTestId('audit-refresh')).toBeVisible()
      await page.getByTestId('audit-actor-filter').fill('e2e-actor')
      await page.getByTestId('audit-controller-filter').fill(controllerId('A'))
      await page.getByTestId('audit-since-filter').fill('2026-07-15T00:00')
      await page.getByTestId('audit-until-filter').fill('2026-07-15T23:59')
      await Promise.all([
        page.waitForResponse((response) => response.url().includes('/center/admin/audit-logs') && response.url().includes('actor=e2e-actor')),
        page.getByTestId('audit-apply').click(),
      ])
      await page.getByTestId('audit-reset').click()
      await expect(page.getByTestId('audit-actor-filter')).toHaveValue('')
      await expect(page.getByTestId('audit-controller-filter')).toHaveValue('')
      await expect(page.getByTestId('audit-since-filter')).toHaveValue('')
      await expect(page.getByTestId('audit-until-filter')).toHaveValue('')
      await clickAndWaitForGet(page, 'audit-refresh', '/center/admin/audit-logs')

      await expect(page.getByTestId('audit-next')).toBeEnabled()
      await Promise.all([
        page.waitForResponse((response) => response.url().includes('/center/admin/audit-logs') && response.url().includes('offset=50')),
        page.getByTestId('audit-next').click(),
      ])
      await expect(page.getByTestId('audit-prev')).toBeEnabled()
      await page.getByTestId('audit-prev').click()
      await expect(page.getByTestId('audit-prev')).toBeDisabled()
    } finally {
      await request.delete(`/api/v1/center/admin/users/${userId}`)
    }
  })

  test('admin controller deletion opens and cancels without touching the two-controller runtime', async ({ page, request }) => {
    test.skip(!(await hasCapability(request, 'controllerHistory')), `${process.env.E2E_MODE} runtime has no controller history capability`)
    await page.goto('/admin')
    await clickAndWaitForGet(page, 'admin-refresh', '/center/admin/controllers')
    if (await openFirstAvailable(page.getByTestId('admin-controller-delete'))) {
      await cancelModal(page, 'admin-delete-cancel')
    }
  })

  test('run-owned role and user execute all safe admin mutations and clean up exactly', async ({ page, request }) => {
    test.setTimeout(60_000)
    const hasAdmin = await hasCapability(request, 'userAdmin') && await hasCapability(request, 'roleAdmin')
    test.skip(!hasAdmin, `${process.env.E2E_MODE} runtime has no user/role admin capability`)
    const suffix = (process.env.E2E_RUN_ID ?? `${Date.now()}`).replace(/[^A-Za-z0-9-]/g, '').slice(-24)
    const roleName = `e2e-role-${suffix}`
    const username = `e2e-user-${suffix}`

    const cleanup = async () => {
      const users = await request.get('/api/v1/center/admin/users')
      if (users.ok()) {
        const body = await users.json() as { data?: Array<{ id: number; username: string }> }
        const user = body.data?.find((item) => item.username === username)
        if (user) await request.delete(`/api/v1/center/admin/users/${user.id}`)
      }
      const roles = await request.get('/api/v1/center/admin/roles')
      if (roles.ok()) {
        const body = await roles.json() as { data?: Array<{ id: number; name: string }> }
        const role = body.data?.find((item) => item.name === roleName)
        if (role) await request.delete(`/api/v1/center/admin/roles/${role.id}`)
      }
    }

    await cleanup()
    try {
      await page.goto('/roles')
      await clickAndWaitForGet(page, 'roles-refresh', '/center/admin/roles')
      await page.getByTestId('role-create').click()
      await cancelModal(page, 'role-cancel')
      await page.getByTestId('role-create').click()
      const roleModal = page.locator('.ant-modal:visible')
      await roleModal.locator('input').first().fill(roleName)
      await roleModal.locator('textarea').fill('Playwright run-owned role')
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'POST' && response.url().endsWith('/center/admin/roles')),
        page.getByTestId('role-confirm').click(),
      ])
      const roleRow = page.getByRole('row').filter({ hasText: roleName })
      await expect(roleRow).toBeVisible()
      await roleRow.getByTestId('role-edit').click()
      const permission = page.getByRole('checkbox').first()
      await permission.click()
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'PUT' && response.url().includes('/center/admin/roles/') && response.url().endsWith('/permissions')),
        page.getByTestId('role-permissions-save').click(),
      ])

      await page.goto('/users')
      await clickAndWaitForGet(page, 'users-refresh', '/center/admin/users')
      await page.getByTestId('user-create').click()
      await cancelModal(page, 'user-cancel')
      await page.getByTestId('user-create').click()
      const createUserModal = page.locator('.ant-modal:visible')
      await createUserModal.locator('input').nth(0).fill(username)
      await createUserModal.locator('input[type="password"]').fill('E2e-Password-2026!')
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'POST' && response.url().endsWith('/center/admin/users')),
        page.getByTestId('user-confirm').click(),
      ])
      const userRow = page.getByRole('row').filter({ hasText: username })
      await expect(userRow).toBeVisible()

      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'PATCH' && response.url().includes('/center/admin/users/')),
        userRow.getByTestId('user-disable').click(),
      ])
      await expect(userRow.getByTestId('user-enable')).toBeVisible()
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'PATCH' && response.url().includes('/center/admin/users/')),
        userRow.getByTestId('user-enable').click(),
      ])

      await userRow.getByTestId('user-reset-password').click()
      await page.locator('.ant-modal:visible input[type="password"]').fill('E2e-New-Password-2026!')
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'PATCH' && response.url().includes('/center/admin/users/')),
        page.getByTestId('user-confirm').click(),
      ])

      await userRow.getByTestId('user-edit-roles').click()
      const rolesModal = page.locator('.ant-modal:visible')
      await rolesModal.locator('.ant-select-selector').click()
      await page.locator('.ant-select-dropdown:visible .ant-select-item-option').filter({ hasText: roleName }).click()
      await page.keyboard.press('Escape')
      await expect(page.locator('.ant-select-dropdown:visible')).toHaveCount(0)
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'PATCH' && response.url().includes('/center/admin/users/')),
        rolesModal.getByTestId('user-confirm').click(),
      ])

      await userRow.getByTestId('user-delete').click()
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'DELETE' && response.url().includes('/center/admin/users/')),
        page.getByTestId('user-confirm').click(),
      ])
      await expect(userRow).toBeHidden()

      await page.goto('/roles')
      const createdRoleRow = page.getByRole('row').filter({ hasText: roleName })
      await expect(createdRoleRow).toBeVisible()
      await createdRoleRow.getByTestId('role-delete').click()
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'DELETE' && response.url().includes('/center/admin/roles/')),
        page.getByTestId('role-confirm').click(),
      ])
      await expect(createdRoleRow).toBeHidden()
    } finally {
      await cleanup()
    }
  })

  test('aggregated restrictions preserve Selector fields while switching and restoring a profile', async ({ page, request }) => {
    await page.goto('/global-connection-ip-restrictions')
    await clickAndWaitForGet(page, 'gcir-refresh', '/global-connection-ip-restrictions')

    await expect(page.getByTestId('gcir-row-open').first()).toBeVisible()
    const controller = controllerId('A')
    const firstRow = page.getByRole('row').filter({ hasText: controller }).filter({ has: page.getByTestId('gcir-row-open') }).first()
    await expect(firstRow).toBeVisible()
    const namespace = (await firstRow.getByRole('cell').nth(1).textContent())?.trim() ?? ''
    await applyTableFilter(page, 'gcir-controller', controller)
    await expect(page.getByTestId('gcir-row-open').first()).toBeVisible()
    await applyTableFilter(page, 'gcir-namespace', namespace)
    await expect(page.getByTestId('gcir-row-open').first()).toBeVisible()

    const nameHeader = page.getByRole('columnheader', { name: /^Name search$/ })
    const filterTrigger = nameHeader.locator('.ant-table-filter-trigger')
    if (await filterTrigger.count()) {
      await filterTrigger.click()
      const search = page.getByTestId('gcir-search')
      await search.fill('__no_plugin_matches__')
      await search.press('Enter')
      await expect(page.getByTestId('gcir-row-open')).toHaveCount(0)
      await filterTrigger.click()
      await page.getByTestId('gcir-search').fill('')
      await page.getByTestId('gcir-search').press('Enter')
    }

    expect(await openFirstAvailable(page.getByTestId('gcir-row-open'))).toBe(true)
    const detailDialog = page.getByRole('dialog')
    await expect(detailDialog).toBeVisible()
    await detailDialog.locator('.ant-modal-footer').getByRole('button', { name: 'Close' }).click()

    const targetRow = page.getByRole('row').filter({ hasText: controllerId('A') }).filter({ has: page.getByTestId('gcir-profile-select') }).first()
    const profile = targetRow.getByTestId('gcir-profile-select')
    const selectorName = `${prefix}-global-ip-selector`
    const before = await configData(request, 'A', selectorName)
    expect(before.spec?.data?.config).toMatchObject({
      active: 'open',
      description: 'E2E active-profile selector',
    })
    try {
      await expect(profile).toBeVisible()
      await profile.click()
      await page.locator('.ant-select-dropdown:visible .ant-select-item-option').filter({ hasText: 'locked' }).click()
      await expect(page.getByRole('dialog')).toBeVisible()
      await page.getByTestId('gcir-profile-cancel').click()
      await expect(page.getByRole('dialog')).toBeHidden()
      await profile.click()
      await page.locator('.ant-select-dropdown:visible .ant-select-item-option').filter({ hasText: 'locked' }).click()
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'PATCH' && response.url().includes('/active-profile')),
        page.getByTestId('gcir-profile-confirm').click(),
      ])
      await expect(profile).toContainText('locked')
      await expect.poll(async () => (await configData(request, 'A', selectorName)).spec?.data?.config?.active).toBe('locked')
      const locked = await configData(request, 'A', selectorName)
      expect(locked.spec?.data?.config?.description).toBe('E2E active-profile selector')
      expect(locked.metadata?.labels?.['edgion.io/e2e-run']).toBe(runId)

      await profile.click()
      await page.locator('.ant-select-dropdown:visible .ant-select-item-option').filter({ hasText: 'open' }).click()
      await Promise.all([
        page.waitForResponse((response) => response.request().method() === 'PATCH' && response.url().includes('/active-profile')),
        page.getByTestId('gcir-profile-confirm').click(),
      ])
    } finally {
      await restoreSelector(request, `${prefix}-global-ip`)
      await expect.poll(async () => (await configData(request, 'A', selectorName)).spec?.data?.config?.active).toBe('open')
      const restored = await configData(request, 'A', selectorName)
      expect(restored.spec?.data?.config).toEqual(before.spec?.data?.config)
      expect(restored.metadata?.labels?.['edgion.io/e2e-run']).toBe(runId)
    }
  })

  test('region routes apply failover to both controllers and restore it', async ({ page, request }) => {
    await page.goto('/region-routes/region')
    await clickAndWaitForGet(page, 'region-refresh', '/region-routes')
    const filter = page.getByRole('combobox').first()
    await filter.fill('__no_region_route_matches__')
    await expect(page.getByTestId('region-failover')).toHaveCount(0)
    await filter.fill('')

    const failover = page.getByTestId('region-failover').first()
    await expect(failover).toBeVisible()
    await expect(failover).toBeEnabled()
    const overrideName = `${prefix}-region-route-override`
    try {
      await failover.click()
      await expect(page.locator('.ant-popover')).toBeVisible()
      const east = page.getByTestId('region-failover-select-east')
      await east.click()
      await page.locator('.ant-select-dropdown:visible .ant-select-item-option').filter({ hasText: 'west' }).click()
      const [response] = await Promise.all([
        page.waitForResponse((item) => item.request().method() === 'POST' && item.url().includes('/center/region-routes/failover')),
        page.getByTestId('region-failover-apply').click(),
      ])
      expect(response.status()).toBe(200)
      expect(await response.json()).toMatchObject({ success: true, data: { modified: 2, failed: 0 } })
      for (const slot of ['A', 'B'] as const) {
        await expect.poll(async () => (await configData(request, slot, overrideName)).spec?.data?.config?.regions?.find((region) => region.name === 'east')?.failoverTo).toBe('west')
        expect((await configData(request, slot, overrideName)).metadata?.labels?.['edgion.io/e2e-run']).toBe(runId)
      }

      // The mutation deliberately waits for federation propagation before it
      // invalidates the topology query. Wait for that lifecycle to complete,
      // then force a fresh read rather than asserting against React Query cache.
      await expect(page.locator('.ant-popover')).toBeHidden()
      await expect.poll(async () => {
        const result = await request.get('/api/v1/center/region-routes')
        const body = await result.json() as { data?: Array<{ controllers?: Record<string, { regions?: Array<{ name?: string; failoverTo?: string }> }> }> }
        return Object.values(body.data?.[0]?.controllers ?? {})[0]?.regions
          ?.find((region) => region.name === 'east')?.failoverTo
      }, { timeout: 20_000 }).toBe('west')
      await page.goto('/region-routes/region')
      await clickAndWaitForGet(page, 'region-refresh', '/region-routes')
      await expect(page.getByText('east → west').first()).toBeVisible()
      await page.getByTestId('region-failover').first().click()
      await page.getByTestId('region-failover-select-east').click()
      await page.locator('.ant-select-dropdown:visible .ant-select-item-option').filter({ hasText: /Normal|Active/ }).click()
      const [restoreResponse] = await Promise.all([
        page.waitForResponse((item) => item.request().method() === 'POST' && item.url().includes('/center/region-routes/failover')),
        page.getByTestId('region-failover-apply').click(),
      ])
      expect(restoreResponse.status()).toBe(200)
      expect(await restoreResponse.json()).toMatchObject({ success: true, data: { modified: 2, failed: 0 } })
    } finally {
      await setRegionFailover(request, '')
      for (const slot of ['A', 'B'] as const) {
        await expect.poll(async () => (await configData(request, slot, overrideName)).spec?.data?.config?.regions?.find((region) => region.name === 'east')?.failoverTo ?? '').toBe('')
        expect((await configData(request, slot, overrideName)).metadata?.labels?.['edgion.io/e2e-run']).toBe(runId)
      }
    }
  })

  test('region route service management, legacy routes, and federation diagnostics expose both controllers', async ({ page, request }) => {
    await page.goto('/region-routes')
    await expect.poll(() => new URL(page.url()).pathname).toBe('/region-routes/region')
    const legacy = await request.get('/api/v1/center/cluster-region-routes')
    expect(legacy.ok()).toBeTruthy()
    expect(legacy.url()).toContain('/api/v1/center/region-routes')
    await page.goto('/region-routes/service')
    const serviceTable = page.getByTestId('region-service-table')
    await expect(serviceTable).toBeVisible()
    await expect(serviceTable).toContainText('2/2')
    await serviceTable.locator('.ant-table-row-expand-icon').first().click()
    await expect(serviceTable).toContainText('controller-a')
    await expect(serviceTable).toContainText('controller-b')
    const usageSearch = page.getByTestId('region-service-search').getByRole('combobox')
    await usageSearch.fill('__no_usage_matches__')
    await expect(page.getByTestId('region-service-table')).toBeHidden()
    await usageSearch.fill('')
    await clickAndWaitForGet(page, 'region-service-refresh', '/center/region-routes')
    await page.getByTestId('region-service-manage-region').first().click()
    await expect.poll(() => new URL(page.url()).pathname).toBe('/region-routes/region')
    await expect(page.getByTestId('region-search').getByRole('combobox')).toHaveValue(`${namespace}/${prefix}-region-route`)
    await page.goto('/region-routes/cluster')
    await expect.poll(() => new URL(page.url()).pathname).toBe('/region-routes/region')

    await page.goto('/federation-diagnostics')
    await expect(page.getByTestId('federation-diagnostics')).toBeVisible()
    await expect(page.getByTestId('federation-watch-table')).toContainText('controller-a')
    await expect(page.getByTestId('federation-watch-table')).toContainText('controller-b')
    await expect(page.getByTestId('federation-region-metadata-table')).toContainText('region-route')
    await expect(page.getByTestId('federation-gir-metadata-table')).toContainText('global-ip')
    await Promise.all([
      page.waitForResponse((response) => response.url().includes('/center/admin/watch-status')),
      page.waitForResponse((response) => response.url().includes('/center/admin/metadata-store')),
      page.getByTestId('federation-diagnostics-refresh').click(),
    ])
  })
})
