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

function expectRegionOutcome(body: unknown) {
  expect(body).toMatchObject({ success: true, data: { modified: 2, failed: 0 } })
  if (process.env.E2E_MODE === 'standalone') {
    const outcomes = (body as { data: { outcomes: Array<{ controllerId: string; state: string }> } }).data.outcomes
    expect(outcomes).toHaveLength(2)
    for (const slot of ['A', 'B'] as const) {
      expect(outcomes).toContainEqual(expect.objectContaining({ controllerId: controllerId(slot), state: 'converged' }))
    }
  }
}

async function expectGlobalFailover(request: APIRequestContext, failoverTo: string) {
  await expect.poll(async () => {
    const result = await request.get('/api/v1/center/region-route-overrides')
    expect(result.ok()).toBeTruthy()
    type Resource = Awaited<ReturnType<typeof configData>>
    const body = await result.json() as { data?: Array<{ namespace: string; name: string; controllers: Record<string, Resource> }> }
    const row = body.data?.find((item) => item.namespace === namespace && item.name === `${prefix}-region-route-override`)
    return (['A', 'B'] as const).map((slot) => {
      const east = row?.controllers[controllerId(slot)]?.spec?.data?.config?.regions?.find((region) => region.name === 'east')
      return east ? east.failoverTo ?? '' : undefined
    })
  }, { timeout: 20_000 }).toEqual([failoverTo, failoverTo])
}

async function setRegionFailover(request: APIRequestContext, failoverTo: string) {
  const response = await request.post('/api/v1/center/region-route-overrides/failover', {
    data: {
      namespace,
      name: `${prefix}-region-route-override`,
      regionName: 'east',
      failoverTo,
    },
  })
  expect(response.status()).toBe(200)
  const body = await response.json() as { success?: boolean; data?: { modified?: number; failed?: number } }
  expectRegionOutcome(body)
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

  test('Controllers page refreshes, filters, reloads, and enters a controller', async ({ page, request }) => {
    // The reload response is terminal: Center waits up to 20 s for the new
    // server_id before answering, so give the whole flow room to breathe.
    test.setTimeout(90_000)
    await page.goto('/controllers')
    await expect(page.getByTestId('controller-enter').first()).toBeVisible()

    await clickAndWaitForGet(page, 'controllers-refresh', '/api/v1/controllers')

    const rows = page.getByTestId('controller-enter')
    const rowCount = await rows.count()
    const search = page.getByTestId('controller-search')
    await search.fill('__no_controller_matches__')
    await search.press('Enter')
    await expect(page.getByTestId('controller-enter')).toHaveCount(0)
    await search.fill(controllerId('A'))
    await search.press('Enter')
    await expect(page.getByTestId('controller-enter').first()).toBeVisible()
    await search.fill('')
    await expect(page.getByTestId('controller-enter')).toHaveCount(rowCount)

    const clusterFilter = page.getByTestId('controller-cluster-filter')
    await clusterFilter.click()
    const clusterOption = page.locator('.ant-select-dropdown:visible .ant-select-item-option').nth(1)
    if (await clusterOption.count()) {
      await clusterOption.click()
      await expect(page.getByTestId('controller-enter').first()).toBeVisible()
    } else {
      await page.keyboard.press('Escape')
    }

    const baseline = new Map<string, string>()
    if (process.env.E2E_MODE === 'standalone') {
      for (const slot of ['A', 'B'] as const) {
        const info = await request.get(`/api/v1/proxy/${controllerPathId(slot)}/api/v1/server-info`)
        expect(info.ok()).toBeTruthy()
        const body = await info.json() as { data: { server_id: string; ready: boolean } }
        expect(body.data.ready).toBe(true)
        expect(body.data.server_id).not.toBe('')
        baseline.set(controllerId(slot), body.data.server_id)
      }
    }

    await page.getByTestId('controller-reload').first().click()
    await cancelModal(page, 'controller-reload-cancel')
    await page.getByTestId('controller-reload').first().click()
    const [reloadResponse] = await Promise.all([
      page.waitForResponse(
        (response) => response.request().method() === 'POST' && /\/controllers\/[^/]+\/reload$/.test(response.url()),
        { timeout: 30_000 },
      ),
      page.getByTestId('controller-reload-confirm').click(),
    ])
    // 200 converged surfaces as a toast; every other terminal state opens a
    // modal the operator must acknowledge — dismiss it so the page is usable.
    expect([200, 202, 409, 502, 503]).toContain(reloadResponse.status())
    if (process.env.E2E_MODE === 'standalone') {
      // This topology has one owning Center and healthy filesystem Controllers.
      // An initiation acknowledgement or timeout is not reload completion.
      expect(reloadResponse.status()).toBe(200)
      const body = await reloadResponse.json() as { data: { state: string; controllerId: string; serverId: string } }
      expect(body.data.state).toBe('converged')
      expect(baseline.has(body.data.controllerId)).toBe(true)
      expect(body.data.serverId).toEqual(expect.any(String))
      expect(body.data.serverId).not.toBe('')
      expect(body.data.serverId).not.toBe(baseline.get(body.data.controllerId))
      const info = await request.get(`/api/v1/proxy/${body.data.controllerId.replaceAll('/', '~')}/api/v1/server-info`)
      expect(info.ok()).toBeTruthy()
      expect(await info.json()).toMatchObject({ data: { server_id: body.data.serverId, ready: true } })
    }
    const outcomeOk = page.getByTestId('controller-reload-outcome-ok')
    if (await outcomeOk.waitFor({ state: 'visible', timeout: 2_000 }).then(() => true, () => false)) {
      await outcomeOk.click()
    }

    await page.getByTestId('controller-enter').first().click()
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
    await page.goto('/controllers')
    await clickAndWaitForGet(page, 'controllers-refresh', '/center/admin/controllers')
    if (await openFirstAvailable(page.getByTestId('controller-delete'))) {
      await cancelModal(page, 'controller-delete-cancel')
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

  test('region routes apply failover to both controllers and restore it', async ({ page, request }) => {
    test.setTimeout(90_000)
    await page.goto('/region-routes/region')
    await clickAndWaitForGet(page, 'region-refresh', '/region-route-overrides')
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
        page.waitForResponse((item) => item.request().method() === 'POST' && item.url().includes('/center/region-route-overrides/failover')),
        page.getByTestId('region-failover-apply').click(),
      ])
      expect(response.status()).toBe(200)
      expectRegionOutcome(await response.json())
      for (const slot of ['A', 'B'] as const) {
        await expect.poll(async () => (await configData(request, slot, overrideName)).spec?.data?.config?.regions?.find((region) => region.name === 'east')?.failoverTo).toBe('west')
        expect((await configData(request, slot, overrideName)).metadata?.labels?.['edgion.io/e2e-run']).toBe(runId)
      }

      await expect(page.locator('.ant-popover')).toBeHidden()
      await expectGlobalFailover(request, 'west')
      await page.goto('/region-routes/region')
      await clickAndWaitForGet(page, 'region-refresh', '/region-route-overrides')
      await expect(page.getByText('east → west').first()).toBeVisible()
      await page.getByTestId('region-failover').first().click()
      await page.getByTestId('region-failover-select-east').click()
      await page.locator('.ant-select-dropdown:visible .ant-select-item-option').filter({ hasText: /^No failover$/ }).click()
      const [restoreResponse] = await Promise.all([
        page.waitForResponse((item) => item.request().method() === 'POST' && item.url().includes('/center/region-route-overrides/failover')),
        page.getByTestId('region-failover-apply').click(),
      ])
      expect(restoreResponse.status()).toBe(200)
      expectRegionOutcome(await restoreResponse.json())
      await expect(page.locator('.ant-popover')).toBeHidden()
      await expectGlobalFailover(request, '')
      await expect(page.getByText('east → west', { exact: true })).toHaveCount(0)
    } finally {
      await setRegionFailover(request, '')
      for (const slot of ['A', 'B'] as const) {
        await expect.poll(async () => (await configData(request, slot, overrideName)).spec?.data?.config?.regions?.find((region) => region.name === 'east')?.failoverTo ?? '').toBe('')
        expect((await configData(request, slot, overrideName)).metadata?.labels?.['edgion.io/e2e-run']).toBe(runId)
      }
    }
  })

})
