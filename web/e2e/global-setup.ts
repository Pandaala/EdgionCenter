import { mkdir } from 'node:fs/promises'
import { resolve } from 'node:path'

export function resolveRunId(now = new Date(), pid = process.pid): string {
  return `resource-ui-${now.toISOString().replace(/[-:.]/g, '').replace('T', '-').replace('Z', 'Z')}-${pid}`
}

export default async function globalSetup(): Promise<void> {
  process.env.E2E_RUN_ID ||= resolveRunId()
  process.env.E2E_ARTIFACT_DIR ||= resolve('test-results', process.env.E2E_RUN_ID)
  await mkdir(process.env.E2E_ARTIFACT_DIR, { recursive: true })
}
