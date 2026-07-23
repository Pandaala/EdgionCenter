import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { expect, test } from '@playwright/test'
import * as yaml from 'js-yaml'
import { RESOURCE_CATALOG, type ResourceCatalogEntry } from '../../src/config/resourceCatalog.ts'
import { readControllerResourceDocument } from '../support/api-oracle.ts'
import { controllerPathId } from '../support/controllers.ts'
import { waitForControllerCapabilities } from '../support/controller-ready.ts'
import { kubectlJson } from '../support/k8s-oracle.ts'

const runId = process.env.E2E_RUN_ID
const controller = controllerPathId('A')
if (!runId) throw new Error('Resource mutation tests require the E2E run')
const prefix = `eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}`
const namespace = `${prefix}-a`
const namespaceB = `${prefix}-b`
const controllerRoot = `/api/v1/proxy/${controller}/api/v1`
const yamlHeaders = { 'content-type': 'application/yaml' }
const mode = process.env.E2E_MODE
const cleanupKindMap = JSON.parse(readFileSync(new URL('../cleanup-kind-map.json', import.meta.url), 'utf8')) as Record<string, string>
const fixtureInventory = JSON.parse(readFileSync(new URL('../fixture-inventory.json', import.meta.url), 'utf8')) as {
  catalogResources: Array<{ kind: string; name: string }>
}
const conditionKinds = new Set([
  'gatewayclass', 'edgiongatewayconfig', 'gateway', 'httproute', 'grpcroute',
  'tcproute', 'udproute', 'tlsroute', 'edgiontls', 'backendtlspolicy',
  'edgionplugins', 'edgionstreamplugins', 'edgionconfigdata', 'edgionacme',
  'linksys', 'edgionbackendtrafficpolicy',
])

const fixtureValues: Record<string, string> = {
  __RUN_ID__: runId,
  __PREFIX__: prefix,
  __NS_A__: namespace,
  __NS_B__: namespaceB,
  __NS_DENIED__: `${prefix}-denied`,
}
const fixtureSource = Object.entries(fixtureValues).reduce(
  (source, [token, value]) => source.replaceAll(token, value),
  readFileSync(new URL('../fixtures/resources/catalog.yaml', import.meta.url), 'utf8'),
)
const fixtureDocuments: Array<Record<string, any>> = []
yaml.loadAll(fixtureSource, (value) => { if (value) fixtureDocuments.push(value as Record<string, any>) })

function fixtureFor(catalog: ResourceCatalogEntry): Record<string, any> {
  const fixture = fixtureInventory.catalogResources.find((item) => item.kind === catalog.displayName)
  if (!fixture) throw new Error(`Fixture inventory is missing ${catalog.displayName}`)
  const document = fixtureDocuments.find((item) => item.kind === catalog.displayName && item.metadata?.name === fixture.name.replace('__PREFIX__', prefix))
  if (!document) throw new Error(`Fixture document is missing ${catalog.displayName}`)
  return structuredClone(document)
}

function mutationDocument(catalog: ResourceCatalogEntry, name: string): Record<string, any> {
  const document = fixtureFor(catalog)
  delete document.status
  const fixtureAnnotations = document.metadata?.annotations
  document.metadata = {
    name,
    ...(catalog.scope === 'namespaced' ? { namespace: document.metadata.namespace } : {}),
    labels: { ...document.metadata.labels, 'edgion.io/e2e-run': runId },
    ...(fixtureAnnotations ? { annotations: structuredClone(fixtureAnnotations) } : {}),
  }
  if (catalog.kind === 'service') {
    for (const key of ['clusterIP', 'clusterIPs', 'healthCheckNodePort', 'ipFamilies', 'ipFamilyPolicy']) delete document.spec?.[key]
    for (const port of document.spec?.ports ?? []) delete port.nodePort
  }
  // The restricted key list intentionally exposes metadata only, so replacement
  // forms cannot infer an existing Secret's immutable type. Use Opaque for this
  // lifecycle test and cover TLS Secret presence separately through the fixture.
  if (catalog.kind === 'secret') document.type = 'Opaque'
  return document
}

function collectionPath(catalog: ResourceCatalogEntry, resourceNamespace?: string): string {
  return catalog.scope === 'cluster'
    ? `${controllerRoot}/cluster/${catalog.kind}`
    : `${controllerRoot}/namespaced/${catalog.kind}/${resourceNamespace}`
}

function itemPath(catalog: ResourceCatalogEntry, resourceNamespace: string | undefined, name: string): string {
  return `${collectionPath(catalog, resourceNamespace)}/${encodeURIComponent(name)}`
}

async function replaceYaml(page: import('@playwright/test').Page, source: string): Promise<void> {
  const container = page.getByTestId('yaml-editor')
  const editor = page.getByTestId('yaml-editor').locator('.monaco-editor')
  // Monaco is split into a large lazy chunk. Repeated real-browser editor
  // workflows can legitimately need longer than the global assertion timeout
  // on a cold local build, so wait for the actual editor rather than treating
  // the Suspense loading state as a product failure.
  await expect(editor).toBeVisible({ timeout: 30_000 })
  await container.evaluate((element, content) => {
    element.dispatchEvent(new CustomEvent('edgion:replace-yaml', { detail: content }))
  }, source)
  await expect(page.getByTestId('yaml-editor').getByText('Syntax Error')).toHaveCount(0)
}

async function yamlEditorDocument(page: import('@playwright/test').Page): Promise<Record<string, any>> {
  const source = await page.getByTestId('yaml-editor').getAttribute('data-yaml-value')
  if (!source) throw new Error('YAML editor did not expose its current document')
  return yaml.load(source) as Record<string, any>
}

async function exerciseEditorRoundTrip(
  page: import('@playwright/test').Page,
  kind: string,
  expected: Record<string, any>,
  annotationValue: string,
): Promise<boolean> {
  const annotationAdd = page.getByTestId('metadata-annotation-add')
  const hasMetadataEditor = await annotationAdd.count() > 0
  if (hasMetadataEditor) {
    await annotationAdd.click()
    const key = page.getByTestId('metadata-annotation-key').last()
    await key.fill('edgion.io/e2e-form')
    await key.blur()
    await page.getByTestId('metadata-annotation-value').last().fill(annotationValue)
  }

  const conditions = page.locator('.ant-tabs-tab[data-node-key="conditions"]')
  await expect(conditions).toHaveCount(conditionKinds.has(kind) ? 1 : 0)
  if (conditionKinds.has(kind)) {
    await conditions.click()
  }
  await page.getByTestId('editor-yaml-tab').click()
  const roundTrip = await yamlEditorDocument(page)
  if (hasMetadataEditor) expect(roundTrip.metadata?.annotations?.['edgion.io/e2e-form']).toBe(annotationValue)
  if (expected.metadata?.annotations) {
    expect(roundTrip.metadata?.annotations).toMatchObject(expected.metadata.annotations)
  }
  if (expected.spec !== undefined) expect(roundTrip.spec).toMatchObject(expected.spec)

  if (conditionKinds.has(kind)) {
    await conditions.click()
  }
  await page.getByTestId('editor-form-tab').click()
  if (hasMetadataEditor) {
    await expect(page.getByTestId('metadata-annotation-key').last()).toHaveValue('edgion.io/e2e-form')
    await expect(page.getByTestId('metadata-annotation-value').last()).toHaveValue(annotationValue)
  }
  return hasMetadataEditor
}

async function expectApiDocument(
  request: import('@playwright/test').APIRequestContext,
  catalog: ResourceCatalogEntry,
  resourceNamespace: string | undefined,
  name: string,
  annotation?: string,
): Promise<void> {
  await expect.poll(async () => {
    try {
      const document = await readControllerResourceDocument(request, controller, catalog.kind, catalog.scope === 'cluster' ? 'Cluster' : 'Namespaced', resourceNamespace, name)
      return annotation ? document.metadata?.annotations?.['edgion.io/e2e-ui'] : document.metadata?.name
    } catch { return undefined }
  }).toBe(annotation ?? name)
  if (mode === 'kubernetes') {
    const document = await kubectlJson({ resource: cleanupKindMap[catalog.displayName], name, namespace: resourceNamespace }) as Record<string, any>
    expect(document.metadata?.name).toBe(name)
    if (annotation) expect(document.metadata?.annotations?.['edgion.io/e2e-ui']).toBe(annotation)
  }
}

async function expectApiAbsent(request: import('@playwright/test').APIRequestContext, path: string): Promise<void> {
  await expect.poll(async () => (await request.get(path)).status()).toBe(404)
}

async function waitForStableResourceVersion(
  request: import('@playwright/test').APIRequestContext,
  catalog: ResourceCatalogEntry,
  resourceNamespace: string | undefined,
  name: string,
): Promise<void> {
  let lastVersion: string | undefined
  let unchangedSince = Date.now()
  await expect.poll(async () => {
    const document = await readControllerResourceDocument(
      request,
      controller,
      catalog.kind,
      catalog.scope === 'cluster' ? 'Cluster' : 'Namespaced',
      resourceNamespace,
      name,
    )
    const version = document.metadata?.resourceVersion as string | undefined
    if (version !== lastVersion) {
      lastVersion = version
      unchangedSince = Date.now()
    }
    return Date.now() - unchangedSince
  }, { intervals: [150], timeout: 10_000 }).toBeGreaterThanOrEqual(750)
}

async function advanceResourceVersion(
  request: import('@playwright/test').APIRequestContext,
  catalog: ResourceCatalogEntry,
  resourceNamespace: string | undefined,
  name: string,
  path: string,
): Promise<string> {
  const writerMarker = `${Date.now()}`
  await expect.poll(async () => {
    const current = await readControllerResourceDocument(
      request,
      controller,
      catalog.kind,
      catalog.scope === 'cluster' ? 'Cluster' : 'Namespaced',
      resourceNamespace,
      name,
    )
    const resourceVersion = current.metadata?.resourceVersion as string | undefined
    if (!resourceVersion) return false
    current.metadata.annotations = {
      ...current.metadata.annotations,
      'edgion.io/e2e-concurrent-writer': writerMarker,
    }
    const response = await request.put(path, {
      data: yaml.dump(current, { lineWidth: -1 }),
      headers: { ...yamlHeaders, 'If-Match': `"${resourceVersion}"` },
    })
    if (response.status() === 409) return false
    expect(response.ok(), await response.text()).toBeTruthy()
    return true
  }, { intervals: [100], timeout: 10_000 }).toBe(true)
  return writerMarker
}

async function openResourcePage(page: import('@playwright/test').Page, catalog: ResourceCatalogEntry): Promise<void> {
  await page.goto(`/controller/${controller}/${catalog.route ?? 'security/dependencies'}`)
  if (catalog.lifecycle === 'restrictedDependency') await page.getByTestId(`${catalog.kind}-tab`).click()
}

async function resourceRow(page: import('@playwright/test').Page, catalog: ResourceCatalogEntry, name: string) {
  const search = page.getByTestId(`${catalog.kind}-search`)
  if (await search.count()) await search.fill(name)
  const row = page.getByRole('row').filter({ hasText: name }).first()
  await expect(row).toBeVisible()
  return row
}

async function createThroughYaml(
  page: import('@playwright/test').Page,
  catalog: ResourceCatalogEntry,
  document: Record<string, any>,
): Promise<void> {
  await page.getByTestId(`${catalog.kind}-create`).click()
  await page.getByTestId('editor-yaml-tab').click()
  await replaceYaml(page, yaml.dump(document, { lineWidth: -1 }))
  const response = page.waitForResponse((value) => value.request().method() === 'POST' && value.url().includes(collectionPath(catalog, document.metadata.namespace)), { timeout: 15_000 })
  await expect(page.getByTestId('editor-submit')).toBeEnabled()
  await page.getByTestId('editor-submit').click()
  const result = await response
  expect(result.ok(), await result.text()).toBeTruthy()
}

test.setTimeout(120_000)

for (const catalog of RESOURCE_CATALOG.values()) {
  test(`real CRUD crosses the browser and API boundary for ${catalog.displayName}`, async ({ page, request }) => {
    if (mode !== 'standalone' && mode !== 'kubernetes') throw new Error('Resource mutation tests require a runtime mode')
    test.info().annotations.push({ type: 'e2e-case', description: `${mode}-A-crud-${catalog.kind}` })
    const verbs = catalog.lifecycle === 'restrictedDependency'
      ? ['list-keys', 'create', 'update', 'delete'] as const
      : ['get', 'list', 'create', 'update', 'delete'] as const
    await waitForControllerCapabilities(request, controller, [{ resourceKind: catalog.kind, verbs }])
    const name = `${prefix}-ui-${catalog.kind}`
    const document = mutationDocument(catalog, name)
    const resourceNamespace = catalog.scope === 'namespaced' ? document.metadata.namespace as string : undefined
    const path = itemPath(catalog, resourceNamespace, name)
    let workflowError: unknown
    try {
      await openResourcePage(page, catalog)
      await createThroughYaml(page, catalog, document)
      await expectApiDocument(request, catalog, resourceNamespace, name)

      // Kubernetes reconcilers may write status immediately after creation,
      // advancing resourceVersion independently of the browser. Wait for every
      // first-class Kubernetes object (and ACME in filesystem mode) to settle,
      // then reload so the editor receives a current CAS precondition.
      if ((mode === 'kubernetes' && catalog.lifecycle === 'firstClass') || catalog.kind === 'edgionacme') {
        await waitForStableResourceVersion(request, catalog, resourceNamespace, name)
        await openResourcePage(page, catalog)
      }

      let row = await resourceRow(page, catalog, name)
      await row.getByTestId(catalog.lifecycle === 'restrictedDependency' ? `${catalog.kind}-row-replace` : `${catalog.kind}-row-edit`).click()
      let formMetadataRoundTrip = await exerciseEditorRoundTrip(page, catalog.kind, document, 'form-update')
      let concurrentWriterMarker: string | undefined
      if (catalog.kind === 'secret') {
        await page.getByRole('button', { name: 'Add Data Entry' }).click()
        await page.getByTestId('secret-data-value').last().fill('form-replacement')
      }
      if (catalog.kind === 'edgionacme') {
        concurrentWriterMarker = await advanceResourceVersion(request, catalog, resourceNamespace, name, path)
        const staleResponse = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path), { timeout: 15_000 })
        await page.getByTestId('editor-submit').click()
        expect((await staleResponse).status()).toBe(409)
        await expect(page.locator('.ant-message-notice-content').filter({ hasText: 'Resource changed; refresh and retry' })).toBeVisible()
        const afterConflict = await readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', resourceNamespace, name)
        expect(afterConflict.metadata?.annotations?.['edgion.io/e2e-concurrent-writer']).toBe(concurrentWriterMarker)
        await page.getByTestId('editor-cancel').click()
        await waitForStableResourceVersion(request, catalog, resourceNamespace, name)
        await openResourcePage(page, catalog)
        row = await resourceRow(page, catalog, name)
        await row.getByTestId(`${catalog.kind}-row-edit`).click()
        formMetadataRoundTrip = await exerciseEditorRoundTrip(page, catalog.kind, document, 'form-update')
      }
      const formResponse = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path), { timeout: 15_000 })
      await expect(page.getByTestId('editor-submit')).toBeEnabled()
      await page.getByTestId('editor-submit').click()
      const formResult = await formResponse
      expect(
        formResult.ok(),
        `Form update failed with ${formResult.status()}: ${await formResult.text()}`,
      ).toBeTruthy()
      await expectApiDocument(request, catalog, resourceNamespace, name)

      const afterForm = await readControllerResourceDocument(request, controller, catalog.kind, catalog.scope === 'cluster' ? 'Cluster' : 'Namespaced', resourceNamespace, name)
      if (formMetadataRoundTrip) expect(afterForm.metadata?.annotations?.['edgion.io/e2e-form']).toBe('form-update')
      if (document.metadata?.annotations) {
        expect(afterForm.metadata?.annotations).toMatchObject(document.metadata.annotations)
      }
      if (concurrentWriterMarker) expect(afterForm.metadata?.annotations?.['edgion.io/e2e-concurrent-writer']).toBe(concurrentWriterMarker)
      if (catalog.lifecycle === 'firstClass' && document.spec !== undefined) expect(afterForm.spec).toMatchObject(document.spec)

      row = await resourceRow(page, catalog, name)
      await row.getByTestId(catalog.lifecycle === 'restrictedDependency' ? `${catalog.kind}-row-replace` : `${catalog.kind}-row-edit`).click()
      await page.getByTestId('editor-yaml-tab').click()
      const current = await readControllerResourceDocument(request, controller, catalog.kind, catalog.scope === 'cluster' ? 'Cluster' : 'Namespaced', resourceNamespace, name)
      const yamlDocument = catalog.lifecycle === 'restrictedDependency'
        ? { ...document, metadata: { ...document.metadata, ...current.metadata } }
        : current
      yamlDocument.metadata.annotations = { ...yamlDocument.metadata.annotations, 'edgion.io/e2e-ui': 'yaml-update' }
      await replaceYaml(page, yaml.dump(yamlDocument, { lineWidth: -1 }))
      const conditions = page.locator('.ant-tabs-tab[data-node-key="conditions"]')
      await expect(conditions).toHaveCount(conditionKinds.has(catalog.kind) ? 1 : 0)
      if (conditionKinds.has(catalog.kind)) {
        await conditions.click()
      }
      await page.getByTestId('editor-form-tab').click()
      if (await page.getByTestId('metadata-annotation-key').count()) {
        await expect(page.getByTestId('metadata-annotation-key').last()).toHaveValue('edgion.io/e2e-ui')
        await expect(page.getByTestId('metadata-annotation-value').last()).toHaveValue('yaml-update')
      }
      await page.getByTestId('editor-yaml-tab').click()
      expect((await yamlEditorDocument(page)).metadata?.annotations?.['edgion.io/e2e-ui']).toBe('yaml-update')
      const yamlResponse = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path), { timeout: 15_000 })
      await page.getByTestId('editor-submit').click()
      expect((await yamlResponse).ok()).toBeTruthy()
      await expectApiDocument(request, catalog, resourceNamespace, name, 'yaml-update')

      if (catalog.lifecycle === 'firstClass') {
        row = await resourceRow(page, catalog, name)
        await row.getByTestId(`${catalog.kind}-row-delete`).click()
        const deleteResponse = page.waitForResponse((value) => value.request().method() === 'DELETE' && value.url().includes(path), { timeout: 15_000 })
        await page.getByTestId('resource-delete-confirm').click()
        expect((await deleteResponse).ok()).toBeTruthy()
        await expectApiAbsent(request, path)
      }
    } catch (error) {
      workflowError = error
    }
    let cleanupError: unknown
    try {
      const cleanup = await request.delete(path)
      if (!cleanup.ok() && cleanup.status() !== 404) cleanupError = new Error(`Exact cleanup failed for ${catalog.displayName}: ${cleanup.status()} ${await cleanup.text()}`)
    } catch (error) {
      cleanupError = error
    }
    if (workflowError) throw workflowError
    if (cleanupError) throw cleanupError
  })
}

test('editor submit replaces an isolated ConfigMap and cleans it exactly', async ({ page, request }) => {
  await waitForControllerCapabilities(request, controller, [{
    resourceKind: 'configmap',
    verbs: ['list-keys', 'create', 'update', 'delete'],
  }])
  const name = `${prefix}-action-configmap`
  const path = `${controllerRoot}/namespaced/configmap/${namespace}`
  const itemPath = `${path}/${name}`
  const source = `apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: ${name}\n  namespace: ${namespace}\ndata:\n  before: e2e\n`
  const created = await request.post(path, { data: source, headers: yamlHeaders })
  expect(created.ok(), await created.text()).toBeTruthy()
  let workflowError: unknown
  try {
    await page.goto(`/controller/${encodeURIComponent(controller)}/security/dependencies`)
    await page.getByTestId('configmap-tab').click()
    await page.getByTestId('configmap-search').fill(name)
    const row = page.getByRole('row').filter({ hasText: name }).first()
    await expect(row).toBeVisible()
    const replace = row.getByTestId('configmap-row-replace')
    await expect(replace).toBeEnabled()
    await replace.click()
    const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(`/configmap/${namespace}/${name}`))
    const submit = page.getByTestId('editor-submit')
    await expect(submit).toBeEnabled()
    await submit.click()
    expect((await response).ok()).toBeTruthy()
  } catch (error) {
    workflowError = error
  }
  const deleted = await request.delete(itemPath)
  const cleanupError = !deleted.ok() && deleted.status() !== 404
    ? new Error(`ConfigMap cleanup failed: ${deleted.status()} ${await deleted.text()}`)
    : undefined
  if (workflowError) throw workflowError
  if (cleanupError) throw cleanupError
})

test('single and batch delete confirmations remove only isolated Services', async ({ page, request }) => {
  await waitForControllerCapabilities(request, controller, [{
    resourceKind: 'service',
    verbs: ['list', 'create', 'delete'],
  }])
  const names = ['single', 'batch-a', 'batch-b'].map((suffix) => `${prefix}-action-${suffix}`)
  const path = `${controllerRoot}/namespaced/service/${namespace}`
  const itemPath = (name: string) => `${path}/${name}`
  for (const name of names) {
    const source = `apiVersion: v1\nkind: Service\nmetadata:\n  name: ${name}\n  namespace: ${namespace}\nspec:\n  ports:\n    - name: http\n      port: 8080\n`
    const created = await request.post(path, { data: source, headers: yamlHeaders })
    expect(created.ok(), await created.text()).toBeTruthy()
  }
  let workflowError: unknown
  try {
    await page.goto(`/controller/${encodeURIComponent(controller)}/services/list`)
    await page.getByTestId('service-search').fill(`${prefix}-action-`)
    const single = page.getByRole('row').filter({ hasText: names[0] }).first()
    const deleteButton = single.getByTestId('service-row-delete')
    await expect(deleteButton).toBeEnabled()
    await deleteButton.click()
    const singleResponse = page.waitForResponse((value) => value.request().method() === 'DELETE' && value.url().includes(`/service/${namespace}/${names[0]}`))
    const deleteConfirm = page.getByTestId('resource-delete-confirm')
    await expect(deleteConfirm).toBeEnabled()
    await deleteConfirm.click()
    expect((await singleResponse).ok()).toBeTruthy()

    for (const name of names.slice(1)) await page.getByRole('row').filter({ hasText: name }).first().locator('input[type="checkbox"]').click()
    const batchDelete = page.getByTestId('service-batch-delete')
    await expect(batchDelete).toBeEnabled()
    await batchDelete.click()
    const batchResponses = names.slice(1).map((name) => page.waitForResponse((value) => value.request().method() === 'DELETE' && value.url().includes(`/service/${namespace}/${name}`)))
    const batchConfirm = page.getByTestId('resource-batch-delete-confirm')
    await expect(batchConfirm).toBeEnabled()
    await batchConfirm.click()
    for (const response of await Promise.all(batchResponses)) expect(response.ok()).toBeTruthy()
  } catch (error) {
    workflowError = error
  }
  let cleanupError: Error | undefined
  for (const name of names) {
    const deleted = await request.delete(itemPath(name))
    if (!deleted.ok() && deleted.status() !== 404 && !cleanupError) {
      cleanupError = new Error(`Service cleanup failed: ${deleted.status()} ${await deleted.text()}`)
    }
  }
  if (workflowError) throw workflowError
  if (cleanupError) throw cleanupError
})
