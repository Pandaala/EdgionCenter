import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { RESOURCE_CATALOG } from '../../src/config/resourceCatalog.ts'
import type { ControllerSlot, E2EMode, ExpectedCase } from '../support/case-ledger.ts'
import { controllerPathId } from '../support/controllers.ts'

const here = dirname(fileURLToPath(import.meta.url))
const e2eRoot = resolve(here, '..')
const modes: E2EMode[] = ['standalone', 'kubernetes']
const controllers: ControllerSlot[] = ['A', 'B']

async function json<T>(path: string): Promise<T> { return JSON.parse(await readFile(path, 'utf8')) as T }

export async function generateCases(): Promise<ExpectedCase[]> {
  const fixtureInventory = await json<{ catalogResources: Array<{ kind: string; name: string }>; states: Array<{ state: string; kind: string; name: string }> }>(resolve(e2eRoot, 'fixture-inventory.json'))
  const policy = await json<{ variants: Array<{ id: string; expectedHttpByMode: Record<E2EMode, number[]> }> }>(resolve(e2eRoot, 'fixtures/policies/authorization.json'))
  const cases: ExpectedCase[] = []
  for (const mode of modes) for (const controller of controllers) {
    const controllerRoot = `/controller/${controllerPathId(controller, false)}`
    for (const fixture of fixtureInventory.catalogResources) {
      const catalog = [...RESOURCE_CATALOG.values()].find(({ displayName }) => displayName === fixture.kind)
      if (!catalog) throw new Error(`Fixture kind absent from resource catalog: ${fixture.kind}`)
      cases.push({
        id: `${mode}-${controller}-resource-${catalog.kind}`, mode, controller,
        page: catalog.route ? `${controllerRoot}/${catalog.route}` : `${controllerRoot}/security/dependencies`, fixture: fixture.name,
        actionTestId: catalog.lifecycle === 'restrictedDependency' ? `${catalog.kind}-row-replace` : `${catalog.kind}-row-view`, expectedHttp: [200], expectedOracle: mode === 'kubernetes' ? 'both' : 'api',
        status: 'expected', durationMs: 0, artifacts: [],
      })
    }
    for (const fixture of fixtureInventory.states) {
      const catalog = [...RESOURCE_CATALOG.values()].find(({ displayName }) => displayName === fixture.kind)
      if (!catalog?.route) throw new Error(`State fixture kind has no route: ${fixture.kind}`)
      cases.push({
      id: `${mode}-${controller}-state-${fixture.state}`, mode, controller, page: `${controllerRoot}/${catalog.route}`,
      fixture: fixture.name,
      actionTestId: fixture.state === 'referencegrant-denied'
        ? 'route-ref-denied'
        : fixture.state === 'referencegrant-allowed'
          ? 'route-ref-granted'
          : `${fixture.kind.toLowerCase()}-row-view`,
      expectedHttp: [200], expectedOracle: mode === 'kubernetes' ? 'both' : 'api', condition: fixture.state,
      status: 'expected', durationMs: 0, artifacts: [],
      })
    }
    for (const variant of policy.variants) cases.push({
      id: `${mode}-${controller}-auth-${variant.id}`, mode, controller, page: '/dashboard', fixture: variant.id,
      actionTestId: variant.id === 'hot-reload' ? 'dashboard-reload' : 'access-retry', expectedHttp: variant.expectedHttpByMode[mode],
      expectedOracle: 'api', condition: variant.id, status: 'expected', durationMs: 0, artifacts: [],
    })
  }
  for (const mode of modes) {
    const controllerRoot = `/controller/${controllerPathId('A', false)}`
    for (const fixture of fixtureInventory.catalogResources) {
      const catalog = [...RESOURCE_CATALOG.values()].find(({ displayName }) => displayName === fixture.kind)
      if (!catalog) throw new Error(`Fixture kind absent from resource catalog: ${fixture.kind}`)
      const page = catalog.route ? `${controllerRoot}/${catalog.route}` : `${controllerRoot}/security/dependencies`
      cases.push({
        id: `${mode}-A-action-${catalog.kind}`, mode, controller: 'A', page, fixture: fixture.name,
        actionTestId: catalog.lifecycle === 'restrictedDependency' ? `${catalog.kind}-row-replace` : `${catalog.kind}-row-edit`,
        expectedHttp: [200], expectedOracle: 'none', status: 'expected', durationMs: 0, artifacts: [],
      })
      cases.push({
        id: `${mode}-A-crud-${catalog.kind}`, mode, controller: 'A', page, fixture: fixture.name,
        actionTestId: 'editor-submit', expectedHttp: [200, 201, 204],
        expectedOracle: mode === 'kubernetes' ? 'both' : 'api', status: 'expected', durationMs: 0, artifacts: [],
      })
    }
  }
  const ids = new Set(cases.map(({ id }) => id)); if (ids.size !== cases.length) throw new Error('Generated duplicate case IDs')
  return cases
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const output = resolve(process.env.E2E_ARTIFACT_DIR ?? 'test-results/inventory', 'expected-cases.json')
  const cases = await generateCases(); await mkdir(dirname(output), { recursive: true }); await writeFile(output, `${JSON.stringify(cases, null, 2)}\n`)
  process.stdout.write(`generated ${cases.length} cases at ${output}\n`)
}
