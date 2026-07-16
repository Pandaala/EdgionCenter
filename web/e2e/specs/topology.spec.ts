import { expect, test } from '@playwright/test'
import { controllerPathId } from '../support/controllers.ts'
test('topology controls are discoverable', async ({ page }) => {
  await page.goto(`/controller/${controllerPathId('A')}/topology`)
  await expect(page.getByTestId('topology-refresh')).toBeVisible()
  await expect(page.getByTestId('topology-namespace-filter')).toBeVisible()
  await expect(page.getByTestId('topology-legend')).toBeVisible()
  await expect(page.getByTestId('topology-canvas')).toBeVisible()
})
