import type { FullResult, Reporter, TestCase, TestResult } from '@playwright/test/reporter'
import { resolve } from 'node:path'
import { generateCases } from '../scripts/generate-cases.ts'
import { assertExactCaseSet, writeLedgerAtomic, type ExpectedCase } from '../support/case-ledger.ts'

export default class CaseLedgerReporter implements Reporter {
  private readonly startedAt = new Date().toISOString()
  private readonly actual = new Map<string, ExpectedCase>()

  onTestEnd(test: TestCase, result: TestResult): void {
    const id = test.annotations.find(({ type }) => type === 'e2e-case')?.description
    if (!id) return
    const durationMs = result.duration
    const status = result.status === 'passed' ? 'passed' : result.status === 'skipped' ? 'skipped' : 'failed'
    const previous = this.actual.get(id)
    const artifacts = result.attachments.flatMap(({ path }) => path ? [path] : [])
    this.actual.set(id, { ...(previous ?? { id } as ExpectedCase), status, durationMs, artifacts })
  }

  async onEnd(result: FullResult): Promise<{ status?: FullResult['status'] }> {
    const mode = process.env.E2E_MODE
    if (mode !== 'standalone' && mode !== 'kubernetes') return {}
    // Focused developer runs intentionally execute a strict subset. The final
    // evidence ledger is emitted only by the unfiltered full matrix.
    if (process.env.E2E_PLAYWRIGHT_GREP) return { status: result.status }
    // Playwright --list and action-only focused runs do not execute any
    // annotated business case, so they must not emit or validate a case ledger.
    if (this.actual.size === 0) return { status: result.status }
    const expected = (await generateCases()).filter((item) => item.mode === mode)
    const actual = [...this.actual.values()].map((item) => ({ ...expected.find(({ id }) => id === item.id)!, ...item }))
    let exact = true
    try { assertExactCaseSet(expected, actual) } catch (error) { exact = false; process.stderr.write(`${String(error)}\n`) }
    const byId = new Map(actual.map((item) => [item.id, item]))
    const cases = expected.map((item) => byId.get(item.id) ?? item)
    const artifactDir = resolve(process.env.E2E_ARTIFACT_DIR ?? `test-results/${mode}`)
    await writeLedgerAtomic(resolve(artifactDir, 'case-ledger.json'), {
      schemaVersion: 1, runId: process.env.E2E_RUN_ID ?? 'missing-run-id', startedAt: this.startedAt,
      finishedAt: new Date().toISOString(), cases,
    })
    if (!exact || cases.some(({ status }) => status !== 'passed')) return { status: 'failed' }
    return { status: result.status }
  }
}
