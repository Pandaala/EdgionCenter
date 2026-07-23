import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { expect, test } from '@playwright/test'
import { RESOURCE_CATALOG } from '../../src/config/resourceCatalog.ts'
import { controllerPathId } from '../support/controllers.ts'
import { waitForControllerCapabilities } from '../support/controller-ready.ts'

const inventory = JSON.parse(readFileSync(new URL('../fixture-inventory.json', import.meta.url), 'utf8')) as {
  catalogResources: Array<{ kind: string; name: string }>
}
const actions = JSON.parse(readFileSync(new URL('../action-inventory.json', import.meta.url), 'utf8')) as {
  batchKinds: string[]; searchExceptions: string[]
}
const runId = process.env.E2E_RUN_ID
const mode = process.env.E2E_MODE
const controllerPath = controllerPathId('A')
if (!runId) throw new Error('Resource action tests require the E2E run')
if (mode !== 'standalone' && mode !== 'kubernetes') throw new Error('Resource action tests require a runtime mode')
const prefix = `eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}`
const render = (value: string) => value.replaceAll('__PREFIX__', prefix)
const formControls: Partial<Record<string, readonly string[]>> = {
  edgiongatewayconfig: ['edgiongatewayconfig-ip-group-add', 'edgiongatewayconfig-ip-group-remove'],
  gateway: ['gateway-address-add', 'gateway-address-remove'],
  httproute: ['httproute-rule-add', 'httproute-rule-remove', 'mirror-tuning-annotations'],
  grpcroute: ['grpcroute-rule-add', 'grpcroute-rule-remove'],
  tcproute: ['streamroute-rule-add', 'streamroute-rule-remove'],
  udproute: ['streamroute-rule-add', 'streamroute-rule-remove'],
  tlsroute: ['streamroute-rule-add', 'streamroute-rule-remove'],
  endpointslice: ['endpointslice-port-add', 'endpointslice-port-remove'],
  backendtlspolicy: ['backendtlspolicy-target-add', 'backendtlspolicy-target-remove'],
  edgionplugins: ['edgionplugins-entry-add', 'edgionplugins-entry-remove'],
  edgionstreamplugins: ['edgionstreamplugins-entry-add', 'edgionstreamplugins-entry-remove'],
  edgionbackendtrafficpolicy: ['edgionbackendtrafficpolicy-target-add', 'edgionbackendtrafficpolicy-target-remove'],
  secret: ['secret-data-add', 'secret-data-remove'],
}

async function exerciseFormControls(page: import('@playwright/test').Page, kind: string): Promise<void> {
  const controls = formControls[kind]
  if (!controls) return
  await page.getByTestId(controls[0]).first().click()
  const remove = page.getByTestId(controls[1]).last()
  await expect(remove).toBeEnabled()
  await remove.click()
  if (kind === 'httproute') {
    await page.getByTestId('mirror-tuning-annotations').getByRole('spinbutton').first().fill('1250')
  }
}

async function installActionRecorder(page: import('@playwright/test').Page): Promise<void> {
  await page.addInitScript(() => {
    const exercised = new Set<string>()
    ;(window as any).__edgionExercisedActions = exercised
    const record = (event: Event) => {
      const target = event.target instanceof Element ? event.target.closest<HTMLElement>('[data-testid]') : null
      if (target?.dataset.testid) exercised.add(target.dataset.testid)
    }
    document.addEventListener('click', record, true)
    document.addEventListener('input', record, true)
    document.addEventListener('change', record, true)
  })
}

async function expectActionsExercised(page: import('@playwright/test').Page, expected: string[]): Promise<void> {
  const exercised = await page.evaluate(() => [...((window as any).__edgionExercisedActions ?? [])] as string[])
  expect(expected.filter((id) => !exercised.includes(id)), `Actions not exercised: ${expected.join(', ')}`).toEqual([])
}

test.setTimeout(60_000)

for (const catalog of RESOURCE_CATALOG.values()) {
  const fixture = inventory.catalogResources.find(({ kind }) => kind === catalog.displayName)
  if (!fixture) throw new Error(`Fixture absent for ${catalog.displayName}`)
  const pagePath = `/controller/${controllerPath}/${catalog.route ?? 'security/dependencies'}`
  const fixtureName = render(fixture.name)

  test(`actions-${catalog.kind}`, async ({ page, request }) => {
    test.info().annotations.push({ type: 'e2e-case', description: `${mode}-A-action-${catalog.kind}` })
    const verbs = catalog.lifecycle === 'restrictedDependency'
      ? ['list-keys', 'create', 'update'] as const
      : ['get', 'list', 'create', 'update', 'delete'] as const
    await waitForControllerCapabilities(request, controllerPath, [{ resourceKind: catalog.kind, verbs }])
    await installActionRecorder(page)
    await page.goto(pagePath)

    if (catalog.lifecycle === 'restrictedDependency') {
      await page.getByTestId(`${catalog.kind}-tab`).click()
      const refresh = page.getByTestId(`${catalog.kind}-refresh`)
      await expect(refresh).toBeEnabled()
      await refresh.click()
      await page.getByTestId(`${catalog.kind}-search`).fill(fixtureName)
      const row = page.getByRole('row').filter({ hasText: fixtureName }).first()
      await expect(row).toBeVisible()
      const replace = row.getByTestId(`${catalog.kind}-row-replace`)
      await expect(replace).toBeEnabled()
      await replace.click()
      await expect(page.getByTestId('editor-form-tab')).toBeVisible()
      await exerciseFormControls(page, catalog.kind)
      await page.getByTestId('editor-yaml-tab').click()
      await page.getByTestId('editor-form-tab').click()
      await page.getByTestId('editor-cancel').click()
      const create = page.getByTestId(`${catalog.kind}-create`)
      await expect(create).toBeEnabled()
      await create.click()
      await page.getByTestId('editor-cancel').click()
      await expectActionsExercised(page, [
        `${catalog.kind}-tab`, `${catalog.kind}-refresh`, `${catalog.kind}-search`,
        `${catalog.kind}-row-replace`, `${catalog.kind}-create`,
        'editor-yaml-tab', 'editor-form-tab', 'editor-cancel',
        ...(formControls[catalog.kind] ?? []),
      ])
      return
    }

    const refresh = page.getByTestId(`${catalog.kind}-refresh`)
    await expect(refresh).toBeEnabled()
    await refresh.click()
    if (!actions.searchExceptions.includes(catalog.kind)) await page.getByTestId(`${catalog.kind}-search`).fill(fixtureName)
    const row = page.getByRole('row').filter({ hasText: fixtureName }).first()
    await expect(row).toBeVisible()
    if (catalog.kind === 'edgionacme') await row.getByTestId('acme-trigger').click()

    const view = row.getByTestId(`${catalog.kind}-row-view`)
    await expect(view).toBeEnabled()
    await view.click()
    await expect(page.getByTestId('editor-form-tab')).toBeVisible()
    await page.getByTestId('editor-yaml-tab').click()
    await page.getByTestId('editor-form-tab').click()
    await page.locator('.ant-modal-close').last().click()

    const edit = row.getByTestId(`${catalog.kind}-row-edit`)
    await expect(edit).toBeEnabled()
    await edit.click()
    await exerciseFormControls(page, catalog.kind)
    await page.getByTestId('editor-cancel').click()
    const deleteButton = row.getByTestId(`${catalog.kind}-row-delete`)
    await expect(deleteButton).toBeEnabled()
    await deleteButton.click()
    await page.getByTestId('resource-delete-cancel').click()
    const create = page.getByTestId(`${catalog.kind}-create`)
    await expect(create).toBeEnabled()
    await create.click()
    await page.getByTestId('editor-cancel').click()

    if (actions.batchKinds.includes(catalog.kind)) {
      await row.locator('input[type="checkbox"]').click()
      const batchDelete = page.getByTestId(`${catalog.kind}-batch-delete`)
      await expect(batchDelete).toBeEnabled()
      await batchDelete.click()
      await page.getByTestId('resource-batch-delete-cancel').click()
    }
    await expectActionsExercised(page, [
      `${catalog.kind}-refresh`,
      ...(!actions.searchExceptions.includes(catalog.kind) ? [`${catalog.kind}-search`] : []),
      `${catalog.kind}-row-view`, `${catalog.kind}-row-edit`, `${catalog.kind}-row-delete`, `${catalog.kind}-create`,
      'editor-yaml-tab', 'editor-form-tab', 'editor-cancel', 'resource-delete-cancel',
      ...(catalog.kind === 'edgionacme' ? ['acme-trigger'] : []),
      ...(actions.batchKinds.includes(catalog.kind) ? [`${catalog.kind}-batch-delete`, 'resource-batch-delete-cancel'] : []),
      ...(formControls[catalog.kind] ?? []),
    ])
  })
}

test('resource list retry recovers after a bounded API failure', async ({ page }) => {
  const pattern = '**/api/v1/proxy/**/api/v1/cluster/gatewayclass*'
  await page.route(pattern, (route) => route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'e2e bounded failure' }) }))
  await page.goto(`/controller/${controllerPath}/infrastructure/gatewayclasses`)
  await expect(page.getByTestId('resource-list-retry')).toBeVisible()
  await page.unroute(pattern)
  await page.getByTestId('resource-list-retry').click()
  await expect(page.getByRole('row').filter({ hasText: render(inventory.catalogResources.find(({ kind }) => kind === 'GatewayClass')!.name) }).first()).toBeVisible()
})
