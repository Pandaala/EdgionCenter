import { createHash } from 'node:crypto'
import { expect, test } from '@playwright/test'
import { controllerPathId } from '../support/controllers.ts'

test('authenticated identity endpoint is available', async ({ request }) => { expect((await request.get('/api/v1/auth/me')).ok()).toBeTruthy() })

test('restricted dependency metadata stays inside configured namespaces', async ({ request }) => {
  test.skip(process.env.E2E_MODE !== 'kubernetes', 'Kubernetes namespace boundary only')
  const runId = process.env.E2E_RUN_ID
  if (!runId) throw new Error('E2E_RUN_ID is required')
  const prefix = `eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}`
  const allowed = new Set([`${prefix}-a`, `${prefix}-b`, `${prefix}-denied`])
  for (const slot of ['A', 'B'] as const) {
    const response = await request.get(`/api/v1/proxy/${controllerPathId(slot)}/api/v1/keys/namespaced/secret`)
    expect(response.ok(), await response.text()).toBeTruthy()
    const body = await response.json() as { data?: Array<{ metadata?: { namespace?: string; name?: string } }> }
    const keys = body.data ?? []
    expect(keys.some(({ metadata }) => metadata?.name === `${prefix}-secret`)).toBeTruthy()
    expect(keys.every(({ metadata }) => metadata?.namespace !== undefined && allowed.has(metadata.namespace))).toBeTruthy()
  }
})

test('standalone restricted user denies administration and honors password and status changes', async ({ request, playwright, browser, baseURL }) => {
  test.skip(process.env.E2E_MODE !== 'standalone' || process.env.E2E_RBAC !== '1', 'Requires standalone with E2E_RBAC=1 and database admin bootstrap')
  test.setTimeout(60_000)
  const suffix = createHash('sha256').update(process.env.E2E_RUN_ID ?? '').digest('hex').slice(0, 8)
  const username = `e2e-restricted-${suffix}`
  const oldPassword = `Old-${suffix}-Password!`
  const newPassword = `New-${suffix}-Password!`
  const created = await request.post('/api/v1/center/admin/users', { data: { username, password: oldPassword, roleIds: [] } })
  expect(created.status()).toBe(201)
  const { data: userId } = await created.json() as { data: number }
  expect(typeof userId).toBe('number')
  const anonymous = await playwright.request.newContext({ baseURL, storageState: { cookies: [], origins: [] } })
  const restrictedContext = await browser.newContext({ baseURL, storageState: { cookies: [], origins: [] } })
  try {
    const login = await anonymous.post('/api/v1/auth/login', { data: { username, password: oldPassword } })
    expect(login.status()).toBe(200)
    const body = await login.json() as { data: { token: string } }
    const headers = { Authorization: `Bearer ${body.data.token}` }
    expect((await anonymous.get('/api/v1/auth/me', { headers })).status()).toBe(200)
    for (const path of ['/api/v1/center/admin/users', '/api/v1/center/admin/roles', '/api/v1/center/admin/audit-logs', `/api/v1/proxy/${controllerPathId('A')}/api/v1/namespaced/service`]) {
      expect((await anonymous.get(path, { headers })).status(), path).toBe(403)
    }
    expect((await request.patch(`/api/v1/center/admin/users/${userId}`, { data: { password: newPassword } })).ok()).toBeTruthy()
    expect((await anonymous.post('/api/v1/auth/login', { data: { username, password: oldPassword } })).status()).toBe(401)
    expect((await anonymous.post('/api/v1/auth/login', { data: { username, password: newPassword } })).status()).toBe(200)
    const page = await restrictedContext.newPage()
    await page.goto('/login')
    await page.getByTestId('login-username').fill(username)
    await page.getByTestId('login-password').fill(newPassword)
    await page.getByTestId('login-submit').click()
    await expect(page).not.toHaveURL(/\/login/)
    await expect(page.getByTestId('user-menu')).toBeVisible()
    for (const name of ['Users', 'Roles', 'Audit Log']) {
      await expect(page.getByRole('button', { name, exact: true })).toHaveCount(0)
    }
    for (const path of ['/users', '/roles', '/audit']) {
      await page.goto(path)
      await expect(page).toHaveURL(new URL('/', baseURL!).href)
      await expect(page.getByTestId('user-create')).toHaveCount(0)
      await expect(page.getByTestId('role-create')).toHaveCount(0)
      await expect(page.getByTestId('audit-refresh')).toHaveCount(0)
    }
    expect((await request.patch(`/api/v1/center/admin/users/${userId}`, { data: { status: 'disabled' } })).ok()).toBeTruthy()
    expect((await anonymous.post('/api/v1/auth/login', { data: { username, password: newPassword } })).status()).toBe(401)
    expect((await request.patch(`/api/v1/center/admin/users/${userId}`, { data: { status: 'active' } })).ok()).toBeTruthy()
    expect((await anonymous.post('/api/v1/auth/login', { data: { username, password: newPassword } })).status()).toBe(200)
  } finally {
    await restrictedContext.close()
    await anonymous.dispose()
    expect((await request.delete(`/api/v1/center/admin/users/${userId}`)).ok()).toBeTruthy()
  }
})
