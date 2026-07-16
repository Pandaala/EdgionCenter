import { expect, test } from '@playwright/test'
import { controllerId, controllerPathId } from '../support/controllers.ts'

test('switches between both seeded controllers', async ({ page }) => {
  for (const slot of ['A', 'B'] as const) {
    await page.goto('/')
    const card = page.locator('.ant-card').filter({
      has: page.getByTestId('controller-card-enter'),
      hasText: controllerId(slot),
    })
    await expect(card).toBeVisible()
    await card.getByTestId('controller-card-enter').click()
    await expect.poll(() => new URL(page.url()).pathname).toBe(`/controller/${controllerPathId(slot)}`)
    await expect(page.getByText(controllerId(slot), { exact: true })).toBeVisible()
  }
})
