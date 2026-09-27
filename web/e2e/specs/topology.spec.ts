import { createHash } from 'node:crypto'
import { expect, test } from '@playwright/test'
import { controllerPathId } from '../support/controllers.ts'
test('topology controls are discoverable', async ({ page }) => {
  await page.goto(`/controller/${controllerPathId('A')}/topology`)
  await expect(page.getByTestId('topology-refresh')).toBeVisible()
  await expect(page.getByTestId('topology-namespace-filter')).toBeVisible()
  await expect(page.getByTestId('topology-legend')).toBeVisible()
  await expect(page.getByTestId('topology-canvas')).toBeVisible()
})


test('topology displays the seeded AI backend and metadata-only credential dependency', async ({ page }) => {
  const runId = process.env.E2E_RUN_ID
  if (!runId) throw new Error('E2E_RUN_ID is required')
  const prefix = `eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}`
  await page.goto(`/controller/${controllerPathId('A')}/topology`)
  const backend = page.locator(`[data-node-testid="topology-node-edgionbackend-${prefix}-ai-backend"]`)
  await expect(backend).toBeVisible()
  await expect(page.locator(`[data-node-testid="topology-node-secret-${prefix}-ai-key"]`)).toBeVisible()
  await backend.click()
  const drawer = page.locator('.ant-drawer-body')
  await expect(drawer).toContainText('credentialPool')
  await expect(drawer).toContainText(`${prefix}-ai-key`)
  await expect(drawer).not.toContainText('fixture-only-not-a-provider-credential')
})
