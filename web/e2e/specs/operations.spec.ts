import { expect, test } from '@playwright/test'
test('region route operations are discoverable', async ({ page }) => {
  await page.goto('/region-routes/region')
  await expect(page.getByTestId('region-refresh')).toBeVisible()
  const failover = page.getByTestId('region-failover')
  if (await failover.count()) {
    await expect(failover.first()).toBeVisible()
  } else {
    await expect(page.locator('.ant-empty')).toBeVisible()
  }
})
