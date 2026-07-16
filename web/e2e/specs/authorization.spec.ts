import { createHash } from 'node:crypto'
import { expect, test } from '@playwright/test'
import { controllerPathId } from '../support/controllers.ts'

test('authenticated identity endpoint is available', async ({ request }) => { expect((await request.get('/api/v1/auth/me')).ok()).toBeTruthy() })

test('restricted dependency metadata stays inside configured namespaces', async ({ request }) => {
  test.skip(process.env.E2E_MODE !== 'kubernetes', 'Kubernetes namespace boundary only')
  const runId = process.env.E2E_RUN_ID
  if (!runId) throw new Error('E2E_RUN_ID is required')
  const prefix = `eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}`
  const allowed = new Set([`${prefix}-a`, `${prefix}-b`, `${prefix}-denied`])
  for (const slot of ['A', 'B'] as const) {
    const response = await request.get(`/api/v1/proxy/${controllerPathId(slot)}/api/v1/keys/namespaced/secret`)
    expect(response.ok(), await response.text()).toBeTruthy()
    const body = await response.json() as { data?: Array<{ metadata?: { namespace?: string; name?: string } }> }
    const keys = body.data ?? []
    expect(keys.some(({ metadata }) => metadata?.name === `${prefix}-secret`)).toBeTruthy()
    expect(keys.every(({ metadata }) => metadata?.namespace !== undefined && allowed.has(metadata.namespace))).toBeTruthy()
  }
})
