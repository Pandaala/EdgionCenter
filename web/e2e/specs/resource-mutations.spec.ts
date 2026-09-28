import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { expect, test } from '@playwright/test'
import * as yaml from 'js-yaml'
import { GLOBAL_RESOURCE_DESCRIPTORS } from '../../src/pages/GlobalResources/globalResourceDescriptors.ts'
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
  'linksys', 'edgionbackendtrafficpolicy', 'edgionbackend',
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

for (const [kind, port] of [['httproute', 8080], ['grpcroute', 8081], ['tcproute', 9000], ['udproute', 9001], ['tlsroute', 8443]] as const) {
  test(`route parent browser clearing restores optional fields for ${kind}`, async ({ page, request }) => {
    const catalog = RESOURCE_CATALOG.get(kind)!
    await waitForControllerCapabilities(request, controller, [{ resourceKind: kind, verbs: ['get', 'list', 'create', 'update', 'delete'] }])
    const name = `${prefix}-${kind}-parent-clear`
    const document = mutationDocument(catalog, name)
    document.spec.parentRefs[0] = { ...document.spec.parentRefs[0], namespace, port }
    const path = itemPath(catalog, namespace, name)
    const created = await request.post(collectionPath(catalog, namespace), { data: yaml.dump(document, { lineWidth: -1 }), headers: yamlHeaders })
    expect(created.ok(), await created.text()).toBeTruthy()
    try {
      await waitForStableResourceVersion(request, catalog, namespace, name)
      await openResourcePage(page, catalog)
      await (await resourceRow(page, catalog, name)).getByTestId(`${kind}-row-edit`).click()
      const parent = page.locator('.ant-card').filter({ has: page.getByText('Gateway 1', { exact: true }) }).last()
      await parent.getByPlaceholder(namespace, { exact: true }).fill('')
      await parent.getByPlaceholder('http-listener', { exact: true }).fill('')
      await page.getByTestId('editor-yaml-tab').click()
      const expected = { name: `${prefix}-gateway`, port }
      expect((await yamlEditorDocument(page)).spec.parentRefs).toEqual([expected])
      await page.getByTestId('editor-form-tab').click()
      const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().endsWith(path))
      await page.getByTestId('editor-submit').click()
      const result = await response
      expect(result.ok(), await result.text()).toBeTruthy()
      const updated = await readControllerResourceDocument(request, controller, kind, 'Namespaced', namespace, name)
      expect(updated.spec.parentRefs).toEqual([expected])
      expect(updated.spec.rules).toEqual(document.spec.rules)
    } finally {
      const cleanup = await request.delete(path)
      expect(cleanup.ok() || cleanup.status() === 404, 'Exact route parent fixture cleanup failed').toBeTruthy()
    }
  })
}

for (const kind of ['httproute', 'grpcroute', 'tcproute', 'udproute', 'tlsroute'] as const) {
  test(`route backend browser clearing restores the owner namespace for ${kind}`, async ({ page, request }) => {
    const catalog = RESOURCE_CATALOG.get(kind)!
    await waitForControllerCapabilities(request, controller, [{ resourceKind: kind, verbs: ['get', 'list', 'create', 'update', 'delete'] }])
    const name = `${prefix}-${kind}-backend-clear`
    const document = mutationDocument(catalog, name)
    const backend = document.spec.rules[0].backendRefs[0]
    Object.assign(backend, { namespace, weight: 0 })
    const expectedBackends = structuredClone(document.spec.rules[0].backendRefs)
    delete expectedBackends[0].namespace
    if (kind === 'httproute') {
      document.spec.rules[0].filters ??= []
      let mirror = document.spec.rules[0].filters.find((filter: { type: string }) => filter.type === 'RequestMirror')
      if (!mirror) {
        mirror = { type: 'RequestMirror', requestMirror: { backendRef: { name: backend.name, port: backend.port }, percent: 10 } }
        document.spec.rules[0].filters.push(mirror)
      }
      mirror.requestMirror.backendRef.namespace = namespace
    }
    const expectedFilters = structuredClone(document.spec.rules[0].filters)
    for (const filter of expectedFilters ?? []) if (filter.type === 'RequestMirror') delete filter.requestMirror.backendRef.namespace
    const path = itemPath(catalog, namespace, name)
    const created = await request.post(collectionPath(catalog, namespace), { data: yaml.dump(document, { lineWidth: -1 }), headers: yamlHeaders })
    expect(created.ok(), await created.text()).toBeTruthy()
    try {
      await waitForStableResourceVersion(request, catalog, namespace, name)
      await openResourcePage(page, catalog)
      await (await resourceRow(page, catalog, name)).getByTestId(`${kind}-row-edit`).click()
      await page.getByRole('textbox', { name: 'Namespace (optional)', exact: true }).fill('')
      if (kind === 'httproute') await page.getByRole('textbox', { name: 'RequestMirror Namespace (optional)', exact: true }).fill('')
      await page.getByTestId('editor-yaml-tab').click()
      expect((await yamlEditorDocument(page)).spec.rules[0].backendRefs).toEqual(expectedBackends)
      await page.getByTestId('editor-form-tab').click()
      const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().endsWith(path))
      await page.getByTestId('editor-submit').click()
      const result = await response
      expect(result.ok(), await result.text()).toBeTruthy()
      const updated = await readControllerResourceDocument(request, controller, kind, 'Namespaced', namespace, name)
      expect(updated.spec.rules[0].backendRefs).toEqual(expectedBackends)
      expect(updated.spec.parentRefs).toEqual(document.spec.parentRefs)
      expect(updated.spec.rules[0].filters).toEqual(expectedFilters)
    } finally {
      const cleanup = await request.delete(path)
      expect(cleanup.ok() || cleanup.status() === 404, 'Exact route backend fixture cleanup failed').toBeTruthy()
    }
  })
}

for (const kind of ['httproute', 'grpcroute'] as const) {
  test(`route policy browser clearing restores optional fields for ${kind}`, async ({ page, request }) => {
    const catalog = RESOURCE_CATALOG.get(kind)!
    await waitForControllerCapabilities(request, controller, [{ resourceKind: kind, verbs: ['get', 'list', 'create', 'update', 'delete'] }])
    const name = `${prefix}-${kind}-policy-clear`
    const document = mutationDocument(catalog, name)
    Object.assign(document.spec.rules[0], {
      timeouts: { request: '30s', backendRequest: '10s' },
      retry: { attempts: kind === 'httproute' ? 1 : 0, backoff: '1s' },
      sessionPersistence: { type: 'Cookie', sessionName: 'SESSION', absoluteTimeout: '1h', ...(mode === 'standalone' ? { strict: false } : {}) },
    })
    const path = itemPath(catalog, namespace, name)
    try {
      await openResourcePage(page, catalog)
      await createThroughYaml(page, catalog, document)
      await expectApiDocument(request, catalog, namespace, name)
      await (await resourceRow(page, catalog, name)).getByTestId(`${kind}-row-edit`).click()
      await expect(page.getByText(/Strict session persistence is supported with FileSystem/)).toBeVisible()
      await expect(page.getByText('Idle Timeout', { exact: true })).toHaveCount(0)
      for (const field of ['Request Timeout', 'Backend Request Timeout', 'Backoff', 'Session Name', 'Absolute Timeout']) {
        await page.getByRole('textbox', { name: field, exact: true }).fill('')
      }
      await page.getByTestId('editor-yaml-tab').click()
      const edited = await yamlEditorDocument(page)
      const expected = { timeouts: {}, retry: { attempts: kind === 'httproute' ? 1 : 0 }, sessionPersistence: { type: 'Cookie', ...(mode === 'standalone' ? { strict: false } : {}) } }
      expect(edited.spec.rules[0]).toMatchObject(expected)
      for (const section of Object.keys(expected)) expect(edited.spec.rules[0][section]).toEqual(expected[section as keyof typeof expected])
      const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path))
      await page.getByTestId('editor-submit').click()
      const result = await response
      expect(result.ok(), await result.text()).toBeTruthy()
      const updated = await readControllerResourceDocument(request, controller, kind, 'Namespaced', namespace, name)
      expect(updated.spec.rules[0].timeouts).toEqual({})
      expect(updated.spec.rules[0].retry).toEqual(expected.retry)
      expect(updated.spec.rules[0].sessionPersistence).toEqual(expected.sessionPersistence)
    } finally {
      const cleanup = await request.delete(path)
      expect(cleanup.ok() || cleanup.status() === 404, 'Exact route policy fixture cleanup failed').toBeTruthy()
    }
  })
}

for (const kind of ['tcproute', 'udproute', 'tlsroute'] as const) {
  test(`stream annotation browser edits preserve current ${kind} controls`, async ({ page, request }) => {
    const catalog = RESOURCE_CATALOG.get(kind)!
    await waitForControllerCapabilities(request, controller, [{ resourceKind: kind, verbs: ['get', 'list', 'create', 'update', 'delete'] }])
    const name = `${prefix}-${kind}-annotations`
    const document = mutationDocument(catalog, name)
    document.metadata.annotations = { 'example.test/preserved': 'yes' }
    const path = itemPath(catalog, namespace, name)
    const expected: Record<string, string> = { ...document.metadata.annotations, 'edgion.io/edgion-stream-plugins': `${namespace}/${prefix}-stream` }
    const formItem = (label: string) => page.locator('.ant-form-item').filter({ has: page.getByText(label, { exact: true }) })
    try {
      await openResourcePage(page, catalog)
      await createThroughYaml(page, catalog, document)
      await expectApiDocument(request, catalog, namespace, name)
      await (await resourceRow(page, catalog, name)).getByTestId(`${kind}-row-edit`).click()
      await page.getByPlaceholder('default/my-stream-plugins').fill(expected['edgion.io/edgion-stream-plugins'])
      if (kind === 'udproute') {
        await expect(page.getByText('TCP Keepalive Idle Time (seconds)', { exact: true })).toHaveCount(0)
      } else {
        await formItem('TCP Keepalive Idle Time (seconds)').locator('input').fill('60')
        expected['edgion.io/tcp-keepalive-time'] = '60'
      }
      if (kind === 'tlsroute') {
        await formItem('Proxy Protocol Version').getByRole('combobox').click()
        await page.locator('.ant-select-item-option-content').filter({ hasText: /^v2$/ }).click()
        await formItem('Max Connection Retries').locator('input').fill('3')
        expected['edgion.io/proxy-protocol'] = 'v2'
        expected['edgion.io/max-connect-retries'] = '3'
      } else {
        await expect(page.getByText('Proxy Protocol Version', { exact: true })).toHaveCount(0)
        await expect(page.getByText('Max Connection Retries', { exact: true })).toHaveCount(0)
      }
      await page.getByTestId('editor-yaml-tab').click()
      const edited = await yamlEditorDocument(page)
      expect(edited.metadata.annotations).toEqual(expected)
      expect(edited.spec).toMatchObject(document.spec)
      const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path))
      await page.getByTestId('editor-submit').click()
      const result = await response
      expect(result.ok(), await result.text()).toBeTruthy()
      const updated = await readControllerResourceDocument(request, controller, kind, 'Namespaced', namespace, name)
      expect(updated.metadata.annotations).toMatchObject(expected)
    } finally {
      const cleanup = await request.delete(path)
      expect(cleanup.ok() || cleanup.status() === 404, 'Exact stream annotation fixture cleanup failed').toBeTruthy()
    }
  })
}

test('gRPC browser form clears optional method predicates without empty names', async ({ page, request }) => {
  const catalog = RESOURCE_CATALOG.get('grpcroute')!
  await waitForControllerCapabilities(request, controller, [{ resourceKind: catalog.kind, verbs: ['get', 'list', 'create', 'update', 'delete'] }])
  const name = `${prefix}-grpc-match`
  const document = mutationDocument(catalog, name)
  const headers = [{ type: 'Exact', name: 'x-tenant', value: 'one' }]
  document.spec.rules[0].matches = [{ method: { type: 'Exact', service: 'demo.Service', method: 'Get' }, headers }]
  const path = itemPath(catalog, namespace, name)
  const save = async (matches: unknown[]) => {
    await page.getByTestId('editor-yaml-tab').click()
    expect((await yamlEditorDocument(page)).spec.rules[0].matches).toEqual(matches)
    const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path))
    await page.getByTestId('editor-submit').click()
    const result = await response
    expect(result.ok(), await result.text()).toBeTruthy()
    const updated = await readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', namespace, name)
    expect(updated.spec.rules[0].matches).toEqual(matches)
  }
  try {
    await openResourcePage(page, catalog)
    await createThroughYaml(page, catalog, document)
    await expectApiDocument(request, catalog, namespace, name)
    await (await resourceRow(page, catalog, name)).getByTestId('grpcroute-row-edit').click()
    await expect(page.getByText(/Stored in configuration, but currently ignored by the Gateway/)).toBeVisible()
    await page.getByRole('textbox', { name: 'gRPC Method', exact: true }).fill('')
    await save([{ method: { type: 'Exact', service: 'demo.Service' }, headers }])
    await (await resourceRow(page, catalog, name)).getByTestId('grpcroute-row-edit').click()
    await page.getByRole('textbox', { name: 'gRPC Service', exact: true }).fill('')
    await save([{ headers }])
    await (await resourceRow(page, catalog, name)).getByTestId('grpcroute-row-edit').click()
    const matchCard = page.locator('.ant-card').filter({ has: page.getByRole('textbox', { name: 'gRPC Method', exact: true }) }).last()
    await matchCard.getByRole('button', { name: 'Delete', exact: true }).click()
    await save([])
  } finally {
    const cleanup = await request.delete(path)
    expect(cleanup.ok() || cleanup.status() === 404, 'Exact gRPC match fixture cleanup failed').toBeTruthy()
  }
})

test('HTTP retry browser form saves only supported response status codes', async ({ page, request }) => {
  const catalog = RESOURCE_CATALOG.get('httproute')!
  await waitForControllerCapabilities(request, controller, [{ resourceKind: catalog.kind, verbs: ['get', 'list', 'create', 'update', 'delete'] }])
  const name = `${prefix}-http-retry`
  const document = mutationDocument(catalog, name)
  document.spec.rules[0].retry = { attempts: 2, codes: [503] }
  const path = itemPath(catalog, namespace, name)
  try {
    await openResourcePage(page, catalog)
    await createThroughYaml(page, catalog, document)
    await expectApiDocument(request, catalog, namespace, name)
    await (await resourceRow(page, catalog, name)).getByTestId('httproute-row-edit').click()
    const codes = page.getByRole('combobox', { name: 'HTTP Retry Status Codes (400-599)' })
    await codes.fill('200')
    await codes.press('Enter')
    await codes.fill('429')
    await codes.press('Enter')
    await page.getByTestId('editor-yaml-tab').click()
    expect((await yamlEditorDocument(page)).spec.rules[0].retry).toEqual({ attempts: 2, codes: [503, 429] })
    const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path))
    await page.getByTestId('editor-submit').click()
    const result = await response
    expect(result.ok(), await result.text()).toBeTruthy()
    const updated = await readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', namespace, name)
    expect(updated.spec.rules[0].retry).toEqual({ attempts: 2, codes: [503, 429] })
  } finally {
    const cleanup = await request.delete(path)
    expect(cleanup.ok() || cleanup.status() === 404, 'Exact HTTP retry fixture cleanup failed').toBeTruthy()
  }
})

test('logical WAF browser form preserves policy references and counts one plugin', async ({ page, request }) => {
  const catalog = RESOURCE_CATALOG.get('edgionplugins')!
  const dataCatalog = RESOURCE_CATALOG.get('edgionconfigdata')!
  await waitForControllerCapabilities(request, controller, [
    { resourceKind: catalog.kind, verbs: ['get', 'list', 'create', 'update', 'delete'] },
    { resourceKind: dataCatalog.kind, verbs: ['get', 'create', 'delete'] },
  ])
  const name = `${prefix}-logical-waf`
  const bundleName = `${prefix}-waf-bundle`
  const policyName = `${prefix}-waf-policy`
  const path = itemPath(catalog, namespace, name)
  const ownedPaths: string[] = []
  try {
    for (const [dataName, data] of [
      [bundleName, { type: 'WafRuleBundle', config: { version: 'one', profile: 'local', provenance: 'e2e', roots: ['main.conf'], rules: [{ name: 'main.conf', content: 'SecRuleEngine On' }] } }],
      [policyName, { type: 'WafPolicy', config: { defaultProfile: 'base', profiles: { base: { bundleRefs: [{ name: bundleName }] } } } }],
    ] as const) {
      const document = mutationDocument(dataCatalog, dataName)
      document.spec = { data }
      const result = await request.post(collectionPath(dataCatalog, namespace), { data: yaml.dump(document), headers: yamlHeaders })
      expect(result.ok(), await result.text()).toBeTruthy()
      ownedPaths.push(itemPath(dataCatalog, namespace, dataName))
    }
    const document = mutationDocument(catalog, name)
    document.spec = { waf: { policyRef: { name: policyName }, activeProfile: 'base', mode: 'detectionOnly', priority: 0,
      requestBody: { inspection: 'prefix', prefixSize: '64KiB' } } }
    await openResourcePage(page, catalog)
    await createThroughYaml(page, catalog, document)
    ownedPaths.push(path)
    await expectApiDocument(request, catalog, namespace, name)
    const row = await resourceRow(page, catalog, name)
    await expect(row.locator('.ant-tag').filter({ hasText: /^WAF$/ })).toBeVisible()
    await expect(row.locator('.ant-badge-count')).toHaveAttribute('title', '1')
    await row.getByTestId('edgionplugins-row-edit').click()
    await exerciseEditorRoundTrip(page, catalog.kind, document, 'waf-update')
    await page.getByRole('textbox').locator('xpath=self::*[@value="64KiB"]').fill('128KiB')
    const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path))
    await page.getByTestId('editor-submit').click()
    const result = await response
    expect(result.ok(), await result.text()).toBeTruthy()
    const submitted = yaml.load(result.request().postData()!) as Record<string, any>
    expect(submitted.spec.waf).toEqual({ ...document.spec.waf, requestBody: { inspection: 'prefix', prefixSize: '128KiB' } })
    const updated = await readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', namespace, name)
    expect(updated.spec.waf).toMatchObject(submitted.spec.waf)
    await (await resourceRow(page, catalog, name)).getByTestId('edgionplugins-row-delete').click()
    const deleted = page.waitForResponse((value) => value.request().method() === 'DELETE' && value.url().includes(path))
    await page.getByTestId('resource-delete-confirm').click()
    expect((await deleted).ok()).toBeTruthy()
    await expectApiAbsent(request, path)
  } finally {
    for (const owned of ownedPaths.reverse()) {
      const cleanup = await request.delete(owned)
      expect(cleanup.ok() || cleanup.status() === 404, 'Exact WAF fixture cleanup failed').toBeTruthy()
    }
  }
})

const typedConfigDataCases = [
  { type: 'RequestAccessUrlAllowList', config: { items: [{ name: 'health', hosts: ['example.com'], paths: [{ type: 'Exact', value: '/health' }] }] }, before: '/health', after: '/ready' },
  { type: 'ProxyProtocolTrust', config: { mode: 'trustedSources', trustedCidrs: ['192.0.2.0/24'] }, before: '192.0.2.0/24', after: '198.51.100.0/24' },
  { type: 'WafRuleBundle', config: { version: 'one', profile: 'local', provenance: 'operator', roots: ['main.conf'], rules: [{ name: 'main.conf', content: 'SecRuleEngine On' }], phraseAssets: [] }, before: 'operator', after: 'repository' },
  { type: 'WafPolicy', config: { defaultProfile: 'main', profiles: { main: { bundleRefs: [{ name: 'rules', optional: false }] } } }, before: 'rules', after: 'updated-rules' },
]

for (const variant of typedConfigDataCases) {
  test(`typed ConfigData browser CRUD preserves ${variant.type}`, async ({ page, request }) => {
    const catalog = RESOURCE_CATALOG.get('edgionconfigdata')!
    await waitForControllerCapabilities(request, controller, [{ resourceKind: catalog.kind, verbs: ['get', 'list', 'create', 'update', 'delete'] }])
    const name = `${prefix}-${variant.type.toLowerCase()}`
    const document = mutationDocument(catalog, name)
    document.spec.data = { type: variant.type, config: variant.config }
    const path = itemPath(catalog, namespace, name)
    try {
      await openResourcePage(page, catalog)
      await createThroughYaml(page, catalog, document)
      await expectApiDocument(request, catalog, namespace, name)
      const row = await resourceRow(page, catalog, name)
      await row.getByTestId('edgionconfigdata-row-edit').click()
      await expect(page.getByTestId('metadata-annotation-add')).toBeVisible()
      await exerciseEditorRoundTrip(page, catalog.kind, document, 'typed-update')
      const field = page.getByRole('textbox').filter({ visible: true })
      const input = field.locator(`xpath=self::*[@value="${variant.before}"]`)
      await expect(input).toHaveCount(1)
      await input.fill(variant.after)
      const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path))
      await page.getByTestId('editor-submit').click()
      expect((await response).ok()).toBeTruthy()
      const updated = await readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', namespace, name)
      expect(updated.metadata.annotations?.['edgion.io/e2e-form']).toBe('typed-update')
      expect(updated.spec.data.type).toBe(variant.type)
      expect(updated.spec.data.config).toEqual(JSON.parse(JSON.stringify(variant.config).replace(variant.before, variant.after)))
      let globalObject: Record<string, any> | undefined
      await expect.poll(async () => {
        const inventory = await request.get('/api/v1/center/global-resources/resources/edgion-config-data', { params: { configDataType: variant.type } })
        expect(inventory.ok(), await inventory.text()).toBeTruthy()
        const body = await inventory.json()
        const group = body.groups.find((entry: any) => entry.key.namespace === namespace && entry.key.name === name)
        globalObject = group?.members.find((member: any) => member.cluster === 'e2e-a')?.object
        return globalObject?.metadata?.annotations?.['edgion.io/e2e-form']
      }, { timeout: 15_000 }).toBe('typed-update')
      if (variant.type === 'ProxyProtocolTrust' || variant.type === 'WafPolicy') {
        expect(globalObject?.spec.data.config).toMatchObject(updated.spec.data.config)
      } else {
        expect(globalObject?.spec.data).not.toHaveProperty('config')
      }
      const descriptor = GLOBAL_RESOURCE_DESCRIPTORS.find((entry) => entry.configDataType === variant.type)!
      await page.goto(descriptor.route)
      await expect(page.getByRole('heading', { name: variant.type, exact: true })).toBeVisible()
      await expect(page.getByRole('row').filter({ hasText: name })).toBeVisible()
      await openResourcePage(page, catalog)
      await (await resourceRow(page, catalog, name)).getByTestId('edgionconfigdata-row-delete').click()
      const deleted = page.waitForResponse((value) => value.request().method() === 'DELETE' && value.url().includes(path))
      await page.getByTestId('resource-delete-confirm').click()
      expect((await deleted).ok()).toBeTruthy()
      await expectApiAbsent(request, path)
    } finally {
      const cleanup = await request.delete(path)
      expect(cleanup.ok() || cleanup.status() === 404, 'Exact ConfigData cleanup failed').toBeTruthy()
    }
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

test('EdgionTls browser form adds and clears optional Gateway attachments', async ({ page, request }) => {
  const catalog = RESOURCE_CATALOG.get('edgiontls')!
  await waitForControllerCapabilities(request, controller, [{
    resourceKind: 'edgiontls', verbs: ['get', 'list', 'create', 'update', 'delete'],
  }])
  const name = `${prefix}-tls-attachment-form`
  const document = mutationDocument(catalog, name)
  const path = itemPath(catalog, namespace, name)
  const created = await request.post(collectionPath(catalog, namespace), {
    data: yaml.dump(document, { lineWidth: -1 }), headers: yamlHeaders,
  })
  expect(created.ok(), await created.text()).toBeTruthy()
  const edit = async () => {
    await waitForStableResourceVersion(request, catalog, namespace, name)
    await openResourcePage(page, catalog)
    await (await resourceRow(page, catalog, name)).getByTestId('edgiontls-row-edit').click()
  }
  const save = async () => {
    const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().endsWith(path))
    await page.getByTestId('editor-submit').click()
    const result = await response
    expect(result.ok(), await result.text()).toBeTruthy()
    return readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', namespace, name)
  }
  try {
    await edit()
    await page.getByRole('button', { name: /Add Gateway/ }).click()
    await page.getByPlaceholder('example-gateway', { exact: true }).fill(`${prefix}-gateway`)
    await page.getByPlaceholder('http-listener', { exact: true }).fill('tls')
    await page.getByPlaceholder('80', { exact: true }).fill('8443')
    await page.getByTestId('editor-yaml-tab').click()
    expect((await yamlEditorDocument(page)).spec.parentRefs).toEqual([{
      group: 'gateway.networking.k8s.io', kind: 'Gateway', namespace,
      name: `${prefix}-gateway`, sectionName: 'tls', port: 8443,
    }])
    await page.getByTestId('editor-form-tab').click()
    const attached = await save()
    expect(attached.spec.parentRefs[0]).toMatchObject({ name: `${prefix}-gateway`, sectionName: 'tls', port: 8443 })
    expect(attached.spec.clientAuth).toEqual(document.spec.clientAuth)

    await edit()
    await page.getByPlaceholder(namespace, { exact: true }).fill('')
    await page.getByPlaceholder('http-listener', { exact: true }).fill('')
    const defaults = await save()
    expect(defaults.spec.parentRefs[0]).not.toHaveProperty('namespace')
    expect(defaults.spec.parentRefs[0]).not.toHaveProperty('sectionName')
    expect(defaults.spec.parentRefs[0].port).toBe(8443)

    await edit()
    const parent = page.locator('.ant-card').filter({ has: page.getByText('Gateway 1', { exact: true }) }).last()
    await parent.getByRole('button', { name: /Delete/ }).click()
    await page.getByTestId('editor-yaml-tab').click()
    expect((await yamlEditorDocument(page)).spec).not.toHaveProperty('parentRefs')
    await page.getByTestId('editor-form-tab').click()
    const detached = await save()
    expect(detached.spec).not.toHaveProperty('parentRefs')
    expect(detached.spec.hosts).toEqual(document.spec.hosts)
    expect(detached.spec.secretRef).toEqual(document.spec.secretRef)
  } finally {
    const cleanup = await request.delete(path)
    expect(cleanup.ok() || cleanup.status() === 404, 'Exact EdgionTls cleanup failed').toBeTruthy()
  }
})

test('filtered batch deletion removes both selected HTTPRoutes and preserves unselected resources', async ({ page, request }) => {
  const catalog = RESOURCE_CATALOG.get('httproute')!
  await waitForControllerCapabilities(request, controller, [{
    resourceKind: 'httproute', verbs: ['get', 'list', 'create', 'delete'],
  }])
  const names = ['visible', 'hidden', 'unselected'].map((suffix) => `${prefix}-filtered-${suffix}`)
  const createdNames: string[] = []
  try {
    for (const name of names) {
      const document = mutationDocument(catalog, name)
      const created = await request.post(collectionPath(catalog, namespace), {
        data: yaml.dump(document, { lineWidth: -1 }), headers: yamlHeaders,
      })
      expect(created.ok(), await created.text()).toBeTruthy()
      createdNames.push(name)
      await waitForStableResourceVersion(request, catalog, namespace, name)
    }
    await openResourcePage(page, catalog)
    const search = page.getByTestId('httproute-search')
    await search.fill(`${prefix}-filtered-`)
    for (const name of names.slice(0, 2)) {
      const row = page.getByRole('row').filter({ has: page.getByText(name, { exact: true }) })
      await row.locator('input[type="checkbox"]').check()
    }
    await search.fill(names[0])
    await expect(page.getByRole('row').filter({ has: page.getByText(names[1], { exact: true }) })).toHaveCount(0)
    await page.getByTestId('httproute-batch-delete').click()
    const confirm = page.getByTestId('resource-batch-delete-confirm')
    await expect(confirm).toBeEnabled()
    const responses = names.slice(0, 2).map((name) => page.waitForResponse((response) =>
      response.request().method() === 'DELETE' && response.url().endsWith(itemPath(catalog, namespace, name)),
    ))
    await confirm.click()
    for (const response of await Promise.all(responses)) expect(response.ok(), await response.text()).toBeTruthy()
    for (const name of names.slice(0, 2)) await expectApiAbsent(request, itemPath(catalog, namespace, name))
    const untouched = await readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', namespace, names[2])
    expect(untouched.metadata?.labels?.['edgion.io/e2e-run']).toBe(runId)
  } finally {
    for (const name of createdNames) {
      const cleanup = await request.delete(itemPath(catalog, namespace, name))
      expect(cleanup.ok() || cleanup.status() === 404, `Exact HTTPRoute cleanup failed: ${name}`).toBeTruthy()
    }
  }
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


const typedLinkSysCases = [
  { type: 'otlp', field: 'linksys-otlp-endpoint', config: { endpoint: 'https://collector.example.test:4317', timeoutMs: 15000, tls: { enabled: false } }, edited: { endpoint: 'https://updated.example.test:4317', timeoutMs: 15000, tls: { enabled: false } }, after: 'https://updated.example.test:4317' },
  { type: 'credentialSource', field: 'linksys-credential-endpoint', config: { provider: { type: 'oauth2ClientCredentials', tokenEndpoint: 'https://issuer.example.test/token', clientAuthentication: { activeSecretRef: { name: 'missing-bootstrap' } }, scopes: ['read'] }, publication: { persist: false, memoryMaxKeys: 100 } }, edited: { provider: { type: 'oauth2ClientCredentials', tokenEndpoint: 'https://updated.example.test/token', clientAuthentication: { activeSecretRef: { name: 'missing-bootstrap' } }, scopes: ['read'] }, publication: { persist: false, memoryMaxKeys: 100 } }, after: 'https://updated.example.test/token' },
  { type: 'redis', field: 'Redis connect timeout', config: { endpoints: [], topology: { mode: 'sentinel', sentinel: { masterName: 'primary', sentinels: ['sentinel.example.test:26379'] } }, db: 255, timeout: { connect: '5s', command: '30s' }, pool: { size: 64 } }, edited: { endpoints: [], topology: { mode: 'sentinel', sentinel: { masterName: 'primary', sentinels: ['sentinel.example.test:26379'] } }, db: 255, timeout: { connect: '7s', command: '30s' }, pool: { size: 64 } }, after: '7s' },
  { type: 'kafka', field: 'Kafka maxTopics', config: { brokers: ['broker.example.test:9092'], maxTopics: 64, maxPendingRecords: 1000, maxPendingBytes: 1048576, lingerMs: 0 }, edited: { brokers: ['broker.example.test:9092'], maxTopics: 32, maxPendingRecords: 1000, maxPendingBytes: 1048576, lingerMs: 0 }, after: '32' },
  { type: 'elasticsearch', field: 'Elasticsearch index prefix', config: { endpoints: ['https://es.example.test:9200'], timeout: { connect: 5000, request: 10000 }, pool: { maxIdlePerHost: 2, idleTimeout: 30000 }, bulk: { batchSize: 100, flushInterval: 1000, maxRetries: 2, backoffMs: 100, maxBodyBytes: 1048576, channelSize: 200 }, index: { prefix: 'before', datePattern: '%Y.%m.%d' } }, edited: { endpoints: ['https://es.example.test:9200'], timeout: { connect: 5000, request: 10000 }, pool: { maxIdlePerHost: 2, idleTimeout: 30000 }, bulk: { batchSize: 100, flushInterval: 1000, maxRetries: 2, backoffMs: 100, maxBodyBytes: 1048576, channelSize: 200 }, index: { prefix: 'after', datePattern: '%Y.%m.%d' } }, after: 'after' },
  { type: 'etcd', field: 'etcd key namespace', config: { endpoints: ['https://etcd.example.test:2379'], namespace: '/before/', timeout: { dial: 5000, request: 10000, keepAlive: 30000 }, keepAlive: { time: 10000, timeout: 5000, permitWithoutStream: false }, autoSyncInterval: 60, maxCallSendSize: 1048576, maxCallRecvSize: 2097152, userAgent: 'center-test', rejectOldCluster: false }, edited: { namespace: '/after/', timeout: { dial: 5000, request: 10000, keepAlive: 30000 }, keepAlive: { time: 10000, timeout: 5000, permitWithoutStream: false }, autoSyncInterval: 60, maxCallSendSize: 1048576, maxCallRecvSize: 2097152, userAgent: 'center-test', rejectOldCluster: false }, after: '/after/' },
  { type: 'httpdns', field: 'HTTP DNS URL template', config: { urlTemplate: 'https://dns.example.test/resolve?name={domain}', response: { kind: 'json', ipPath: 'data.ips', ttlPath: 'ttl' }, fallback: { type: 'dns', servers: ['1.1.1.1', '[::1]:5353'] }, connection: { timeoutMs: 2000, tls: { enabled: false } } }, edited: { urlTemplate: 'https://dns2.example.test/resolve?name={domain}', response: { kind: 'json', ipPath: 'data.ips', ttlPath: 'ttl' }, fallback: { type: 'dns', servers: ['1.1.1.1', '[::1]:5353'] }, connection: { timeoutMs: 2000, tls: { enabled: false } } }, after: 'https://dns2.example.test/resolve?name={domain}' },
  { type: 'webhook', field: 'Webhook URL', config: { target: { url: 'https://hook.example.test' }, timeoutMs: 2000, request: { method: { template: 'POST' } }, maxResponseBytes: 4096, success: { body: [{ pointer: '/ok', equals: true }] }, retry: { maxRetries: 2, retryDelayMs: 50, maxDelayMs: 100 } }, edited: { target: { url: 'https://hook2.example.test' }, timeoutMs: 2000, request: { method: { template: 'POST' } }, maxResponseBytes: 4096, success: { body: [{ pointer: '/ok', equals: true }] }, retry: { maxRetries: 2, retryDelayMs: 50, maxDelayMs: 100 } }, after: 'https://hook2.example.test' },
]

for (const variant of typedLinkSysCases) {
 test(`typed LinkSys browser CRUD preserves ${variant.type}`, async ({ page, request }) => {
  const catalog = RESOURCE_CATALOG.get('linksys')!
  await waitForControllerCapabilities(request, controller, [{ resourceKind: 'linksys', verbs: ['get', 'list', 'create', 'update', 'delete'] }])
  const name = `${prefix}-${variant.type.toLowerCase()}-ui`
  const document = mutationDocument(catalog, name)
  document.spec = { type: variant.type, config: variant.config }
  const path = itemPath(catalog, namespace, name)
  try {
    await openResourcePage(page, catalog)
    await createThroughYaml(page, catalog, document)
    await expectApiDocument(request, catalog, namespace, name)
    await (await resourceRow(page, catalog, name)).getByTestId('linksys-row-edit').click()
    const field = variant.field.startsWith('linksys-') ? page.getByTestId(variant.field) : page.getByLabel(variant.field, { exact: true })
    await expect(field).toBeVisible()
    await field.fill(variant.after)
    await page.getByTestId('editor-yaml-tab').click()
    expect((await yamlEditorDocument(page)).spec.config).toMatchObject(variant.edited)
    await page.getByTestId('editor-form-tab').click()
    const saved = page.waitForResponse((response) => response.request().method() === 'PUT' && response.url().includes(path))
    await page.getByTestId('editor-submit').click()
    expect((await saved).ok()).toBeTruthy()
    const stored = await readControllerResourceDocument(request, controller, 'linksys', 'Namespaced', namespace, name)
    expect(stored.spec.type).toBe(variant.type)
    expect(stored.spec.config).toMatchObject(variant.edited)
    await (await resourceRow(page, catalog, name)).getByTestId('linksys-row-delete').click()
    const deleted = page.waitForResponse((response) => response.request().method() === 'DELETE' && response.url().includes(path))
    await page.getByTestId('resource-delete-confirm').click()
    expect((await deleted).ok()).toBeTruthy()
    await expectApiAbsent(request, path)
  } finally {
    const cleanup = await request.delete(path)
    expect(cleanup.ok() || cleanup.status() === 404, 'Exact typed LinkSys fixture cleanup failed').toBeTruthy()
  }
})

}

test('AI backend traffic policy preserves supported sections through the browser form', async ({ page, request }) => {
  test.skip(mode !== 'standalone', 'Native Controller policy editing coverage')
  const catalog = RESOURCE_CATALOG.get('edgionbackendtrafficpolicy')!
  const name = `${prefix}-ai-resilience-policy`
  const document = mutationDocument(catalog, name)
  document.spec = {
    targetRefs: [{ group: 'edgion.io', kind: 'EdgionBackend', name: `${prefix}-ai-backend` }],
    outlierDetection: { consecutiveErrors: 3, ejectionTime: '30s', maxEjectionPercent: 50 },
    retryConstraint: { budget: { percent: 10, interval: '10s' } },
    circuitBreaker: { maxParallelRequests: 20 },
    connection: { connectTimeout: '3s' },
  }
  const path = itemPath(catalog, namespace, name)
  let created = false
  try {
    await openResourcePage(page, catalog)
    await createThroughYaml(page, catalog, document)
    created = true
    await expectApiDocument(request, catalog, namespace, name)
    const row = await resourceRow(page, catalog, name)
    await expect(row).not.toContainText('RoundRobin')
    await row.getByTestId('edgionbackendtrafficpolicy-row-edit').click()
    await expect(page.getByText(/AI backends do not support load balancing/)).toBeVisible()
    for (const label of ['Load Balancer (optional)', 'Active Health Check (optional)', 'Dynamic Upstream Authority (optional)']) {
      await expect(page.getByRole('switch', { name: label, exact: true })).toBeDisabled()
    }
    await exerciseEditorRoundTrip(page, catalog.kind, document, 'ai-policy-edit')
    const response = page.waitForResponse((value) => value.request().method() === 'PUT' && value.url().includes(path))
    await page.getByTestId('editor-submit').click()
    expect((await response).ok()).toBeTruthy()
    const current = await readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', namespace, name)
    expect(current.spec).toMatchObject(document.spec)
    expect(current.metadata.annotations['edgion.io/e2e-form']).toBe('ai-policy-edit')
    for (const section of ['loadBalancer', 'healthCheck', 'upstreamAuthority']) expect(current.spec[section] ?? undefined).toBeUndefined()
  } finally {
    if (created) {
      const current = await readControllerResourceDocument(request, controller, catalog.kind, 'Namespaced', namespace, name)
      expect(current.metadata.labels['edgion.io/e2e-run']).toBe(runId)
      const deleted = await request.delete(path, { headers: { 'If-Match': `"${current.metadata.resourceVersion}"` } })
      expect(deleted.ok()).toBeTruthy()
      await expectApiAbsent(request, path)
    }
  }
})
