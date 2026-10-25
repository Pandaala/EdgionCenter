import { expect, test } from '@playwright/test'
import { controllerPathId } from '../support/controllers.ts'

test('YAML editor loads and validates without external network access', async ({ page, baseURL }) => {
  test.setTimeout(60_000)
  const origin = new URL(baseURL!).origin
  const external: string[] = []
  await page.route('**/*', async (route) => {
    const url = new URL(route.request().url())
    if (['http:', 'https:'].includes(url.protocol) && url.origin !== origin) {
      external.push(url.origin)
      await route.abort()
    } else await route.continue()
  })
  await page.goto(`/controller/${controllerPathId('A')}/system/config`)
  await page.getByTestId('edgiongatewayconfig-create').click()
  await page.getByTestId('editor-yaml-tab').click()
  const container = page.getByTestId('yaml-editor')
  await expect(container.locator('.monaco-editor')).toBeVisible({ timeout: 30_000 })
  await container.evaluate((element) => element.dispatchEvent(new CustomEvent('edgion:replace-yaml', { detail: 'spec: [' })))
  await expect(container.getByText('Syntax Error', { exact: true })).toBeVisible()
  await container.evaluate((element) => element.dispatchEvent(new CustomEvent('edgion:replace-yaml', {
    detail: 'apiVersion: edgion.io/v1alpha1\nkind: EdgionGatewayConfig\nmetadata: {name: offline-review}\nspec: {requestBody: {maxBodySize: 32MiB}}',
  })))
  await expect(container.getByText('Syntax Error', { exact: true })).toHaveCount(0)
  expect(external).toEqual([])
  await page.getByTestId('editor-cancel').click()
})
