import { expect, test } from '@playwright/test'
import { controllerId, controllerPathId } from '../support/controllers.ts'

test('switches between both seeded controllers', async ({ page }) => {
  for (const slot of ['A', 'B'] as const) {
    await page.goto('/controllers')
    const row = page.locator('.ant-table-row').filter({
      has: page.getByTestId('controller-enter'),
      hasText: controllerId(slot),
    })
    await expect(row).toBeVisible()
    await row.getByTestId('controller-enter').click()
    await expect.poll(() => new URL(page.url()).pathname).toBe(`/controller/${controllerPathId(slot)}`)
    await expect(page.getByText(controllerId(slot), { exact: true })).toBeVisible()
  }
})
