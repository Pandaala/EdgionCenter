import { createHash } from 'node:crypto'
import { expect, test } from '@playwright/test'

test('local provider account edits preserve labels and reject concurrent revisions', async ({ page, request }) => {
  test.skip(process.env.E2E_MODE !== 'standalone', 'Uses the isolated standalone artifact database')
  // Metadata-only: provider adapters are disabled, and no inspection/DNS call is made.
  // Account deletion is not exposed; this record remains in the run-owned database.
  const name = `e2e-cloud-${createHash('sha256').update(process.env.E2E_RUN_ID!).digest('hex').slice(0, 8)}`
  const path = `/api/v1/center/cloud/provider-accounts/${name}`
  const desired = { displayName: 'Original', labels: { team: 'edge' }, managementPolicy: 'observe_only', provider: 'cloudflare', scope: { provider: 'cloudflare', accountId: '0123456789abcdef0123456789abcdef' }, credentialSource: { type: 'static_secret', credentialRef: 'e2e/unresolved-reference' } }
  const created = await request.post('/api/v1/center/cloud/provider-accounts', { data: { accountId: name, desired } })
  expect(created.status()).toBe(201)
  await page.goto('/cloud/provider-accounts')
  const row = page.getByRole('row').filter({ hasText: name })
  await row.getByRole('button', { name: 'Edit', exact: true }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByLabel('Display Name')).toHaveValue('Original')
  await dialog.getByLabel('Display Name').fill('Browser draft')
  const current = await request.get(path)
  expect(current.ok()).toBeTruthy()
  const concurrent = await request.put(path, { headers: { 'If-Match': current.headers().etag }, data: { desired: { ...desired, displayName: 'Concurrent', labels: { team: 'other' } } } })
  expect(concurrent.ok()).toBeTruthy()
  const failed = page.waitForResponse(response => response.request().method() === 'PUT' && response.url().endsWith(path))
  await dialog.getByRole('button', { name: 'Save', exact: true }).click()
  expect((await failed).status()).toBe(412)
  await expect(dialog.getByLabel('Display Name')).toHaveValue('Browser draft')
  const afterConflict = await request.get(path)
  expect(await afterConflict.json()).toMatchObject({ data: { displayName: 'Concurrent', labels: { team: 'other' } } })
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click()
  await row.getByRole('button', { name: 'Edit', exact: true }).click()
  await expect(dialog.getByLabel('Display Name')).toHaveValue('Concurrent')
  await dialog.getByLabel('Display Name').fill('Saved browser edit')
  const saved = page.waitForResponse(response => response.request().method() === 'PUT' && response.url().endsWith(path))
  await dialog.getByRole('button', { name: 'Save', exact: true }).click()
  expect((await saved).ok()).toBeTruthy()
  const afterSave = await request.get(path)
  expect(await afterSave.json()).toMatchObject({ data: { displayName: 'Saved browser edit', labels: { team: 'other' } } })
})
