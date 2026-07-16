import { expect, test } from '@playwright/test'
import { controllerPathId } from '../support/controllers.ts'
test('ops dashboard exposes the resource counts it owns', async ({ page }) => {
  await page.goto(`/controller/${controllerPathId('A')}`)
  for (const kind of ['gatewayclass', 'edgionplugins', 'edgionstreamplugins', 'edgionconfigdata', 'edgiongatewayconfig', 'linksys', 'referencegrant']) {
    await expect(page.getByTestId(`count-${kind}`)).toBeVisible()
  }
})
