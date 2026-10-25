import { expect, test } from '@playwright/test'
import { centerMenu, controllerMenu, type CenterCapability, type MenuNode } from '../../src/components/shell/menuConfig.tsx'
import { resolveServerDiscovery } from '../../src/utils/discovery.ts'
import { controllerPathId } from '../support/controllers.ts'

function leaves(nodes: MenuNode[], capabilities: Partial<Record<CenterCapability, boolean>>, parentEnabled = true): Array<{ path: string; enabled: boolean }> {
  return nodes.flatMap((node) => {
    const enabled = parentEnabled && (!node.requiredCapability || capabilities[node.requiredCapability] === true)
    return node.kind === 'item' ? [{ path: node.path, enabled }] : leaves(node.children, capabilities, enabled)
  })
}

test('visits every Center and dual-Controller page in menu order', async ({ page, request }) => {
  test.skip(process.env.E2E_MODE !== 'standalone', 'This ordered inventory uses the standalone administrator permissions')
  test.setTimeout(180_000)
  const response = await request.get('/api/v1/server-info')
  expect(response.ok()).toBeTruthy()
  const { capabilities } = resolveServerDiscovery(await response.json())
  const visits: Array<{ requested: string; actual: string; enabled: boolean }> = []
  const visit = async (path: string, enabled: boolean) => {
    await page.goto(path, { waitUntil: 'networkidle' })
    const expected = enabled ? path.replace(/\/$/, '') || '/' : '/'
    await expect.poll(() => new URL(page.url()).pathname.replace(/\/$/, '') || '/').toBe(expected)
    await expect(page.getByRole('main')).toBeVisible()
    await expect(page.locator('.ant-alert-error')).toHaveCount(0)
    visits.push({ requested: path, actual: new URL(page.url()).pathname, enabled })
  }
  for (const leaf of leaves(centerMenu.flatMap((section) => section.children), capabilities ?? {})) {
    await visit(leaf.path, leaf.enabled)
  }
  for (const slot of ['A', 'B'] as const) {
    const root = `/controller/${controllerPathId(slot)}`
    for (const leaf of leaves(controllerMenu.flatMap((section) => section.children), {})) {
      await visit(`${root}${leaf.path === '/' ? '' : leaf.path}`, true)
    }
  }
  await test.info().attach('ordered-menu-visits', { body: JSON.stringify(visits, null, 2), contentType: 'application/json' })
})
