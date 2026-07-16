import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { expect, test } from '@playwright/test'
import { RESOURCE_CATALOG } from '../../src/config/resourceCatalog.ts'
import { generateCases } from '../scripts/generate-cases.ts'
import { pollResource, readControllerProcessedResource, readControllerResource } from '../support/api-oracle.ts'
import { kubectlJson } from '../support/k8s-oracle.ts'
import { controllerId, controllerPathId } from '../support/controllers.ts'

const cleanupKindMap = JSON.parse(readFileSync(new URL('../cleanup-kind-map.json', import.meta.url), 'utf8')) as Record<string, string>
const inventory = JSON.parse(readFileSync(new URL('../fixture-inventory.json', import.meta.url), 'utf8')) as {
  catalogResources: Array<{ kind: string; name: string; namespace?: string }>
  states: Array<{ state: string; kind: string; name: string; namespace?: string }>
}

const mode = process.env.E2E_MODE
if (mode !== 'standalone' && mode !== 'kubernetes') throw new Error('Generated runtime cases require standalone or kubernetes mode')
const runId = process.env.E2E_RUN_ID
if (!runId) throw new Error('E2E_RUN_ID is required')
const prefix = `eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}`
const values: Record<string, string> = { __RUN_ID__: runId, __PREFIX__: prefix, __NS_A__: `${prefix}-a`, __NS_B__: `${prefix}-b`, __NS_DENIED__: `${prefix}-denied` }
const render = (value?: string): string | undefined => value && Object.entries(values).reduce((text, [token, replacement]) => text.replaceAll(token, replacement), value)
const fixtureRows = [...inventory.catalogResources, ...inventory.states]
const expectedCases = (await generateCases()).filter((item) => item.mode === mode)

for (const expectedCase of expectedCases.filter(({ id }) => !id.includes('-auth-') && !id.includes('-action-') && !id.includes('-crud-'))) {
  test(expectedCase.id, async ({ page, request }) => {
    test.info().annotations.push({ type: 'e2e-case', description: expectedCase.id })
    const row = fixtureRows.find((candidate) => candidate.name === expectedCase.fixture && (!expectedCase.condition || ('state' in candidate && candidate.state === expectedCase.condition)))
    if (!row) throw new Error(`Fixture metadata is unavailable: ${expectedCase.fixture}`)
    const catalog = [...RESOURCE_CATALOG.values()].find(({ displayName }) => displayName === row.kind)
    if (!catalog) throw new Error(`Catalog metadata is unavailable: ${row.kind}`)
    const controller = controllerId(expectedCase.controller)
    const name = render(row.name)!
    const namespace = render('namespace' in row ? row.namespace : undefined)

    await page.goto(expectedCase.page)
    if (catalog.lifecycle === 'restrictedDependency') {
      await page.getByTestId(`${catalog.kind}-tab`).click()
      await page.getByTestId(`${catalog.kind}-search`).fill(name)
    }
    const tableRow = page.getByRole('row').filter({ hasText: name }).first()
    await expect(tableRow).toBeVisible()
    if (expectedCase.actionTestId === 'route-ref-denied' || expectedCase.actionTestId === 'route-ref-granted') {
      await tableRow.getByTestId(`${catalog.kind}-row-view`).click()
      await page.getByTestId('editor-conditions-tab').click()
      await expect(page.getByTestId(expectedCase.actionTestId).first()).toBeVisible()
    } else {
      await expect(tableRow.getByTestId(expectedCase.actionTestId).first()).toBeVisible()
    }
    const scope = catalog.scope === 'cluster' ? 'Cluster' : 'Namespaced'
    const read = () => readControllerResource(request, controller, catalog.kind, scope, namespace, name)
    const snapshot = await read()
    expect(snapshot).toBeTruthy()
    if (expectedCase.condition) {
      const readProcessed = () => readControllerProcessedResource(request, controller, catalog.kind, namespace, name)
      const expectedCondition: Record<string, { type: string; status: string; reason?: string }> = {
        accepted: { type: 'Accepted', status: 'True' },
        rejected: { type: 'Accepted', status: 'False' },
        unresolved: { type: 'ResolvedRefs', status: 'False', reason: 'BackendNotFound' },
        'referencegrant-denied': { type: 'ResolvedRefs', status: 'False', reason: 'RefNotPermitted' },
        'referencegrant-allowed': { type: 'ResolvedRefs', status: 'True' },
        conflict: { type: 'Conflicted', status: 'True', reason: 'LostOldestWins' },
      }
      const wanted = expectedCondition[expectedCase.condition]
      await pollResource(request, readProcessed, ({ conditions }) => conditions.some((condition) => {
        const item = condition as { type?: string; status?: string; reason?: string }
        return item.type === wanted.type && item.status === wanted.status && (!wanted.reason || item.reason === wanted.reason)
      }))
    }

    if (mode === 'kubernetes') {
      const value = await kubectlJson({ resource: cleanupKindMap[row.kind], name, namespace }) as { metadata?: { name?: string; labels?: Record<string, string> } }
      expect(value.metadata?.name).toBe(name)
      expect(value.metadata?.labels?.['edgion.io/e2e-run']).toBe(runId)
    }
  })
}

for (const expectedCase of expectedCases.filter(({ id }) => id.includes('-auth-'))) {
  test(expectedCase.id, async ({ request }) => {
    test.info().annotations.push({ type: 'e2e-case', description: expectedCase.id })
    let response
    switch (expectedCase.condition) {
      case 'default': response = await request.get('/api/v1/auth/me'); break
      case 'read-only': response = await request.get('/api/v1/controllers'); break
      case 'write': response = await request.post(`/api/v1/controllers/${controllerPathId(expectedCase.controller)}/reload`); break
      case 'hot-reload': {
        response = await request.post(`/api/v1/proxy/${controllerPathId(expectedCase.controller)}/api/v1/reload`)
        for (let attempt = 0; response.status() === 503 && attempt < 15; attempt += 1) {
          await new Promise((resolve) => setTimeout(resolve, 1_000))
          response = await request.post(`/api/v1/proxy/${controllerPathId(expectedCase.controller)}/api/v1/reload`)
        }
        break
      }
      case 'missing-proxy-access': response = await request.get('/api/v1/proxy/not-authorized/api/v1/access'); break
      case 'old-controller-404': response = await request.get(`/api/v1/proxy/${controllerPathId(expectedCase.controller)}-offline/api/v1/access`); break
      default: throw new Error(`Unknown authorization case: ${expectedCase.condition}`)
    }
    expect(expectedCase.expectedHttp).toContain(response.status())
  })
}
