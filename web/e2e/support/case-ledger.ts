import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'

export type E2EMode = 'standalone' | 'kubernetes'
export type ControllerSlot = 'A' | 'B'
export interface ExpectedCase {
  id: string; mode: E2EMode; controller: ControllerSlot; page: string; fixture: string
  actionTestId: string; expectedHttp: number[]; expectedOracle: 'api' | 'kubernetes' | 'both' | 'none'
  status: 'expected' | 'passed' | 'failed' | 'skipped'; durationMs: number; artifacts: string[]; condition?: string | null
}
export interface CaseLedger { schemaVersion: 1; runId: string; startedAt: string; finishedAt: string; cases: ExpectedCase[] }

function assertUniqueIds(cases: ExpectedCase[]): void {
  const ids = new Set<string>()
  for (const item of cases) {
    if (ids.has(item.id)) throw new Error(`Duplicate case id: ${item.id}`)
    ids.add(item.id)
  }
}

export function assertExactCaseSet(expected: ExpectedCase[], actual: ExpectedCase[]): void {
  assertUniqueIds(expected); assertUniqueIds(actual)
  const expectedIds = new Set(expected.map(({ id }) => id)); const actualIds = new Set(actual.map(({ id }) => id))
  const missing = [...expectedIds].filter((id) => !actualIds.has(id)); const extra = [...actualIds].filter((id) => !expectedIds.has(id))
  if (missing.length || extra.length) throw new Error(`Case set mismatch; missing=${missing.join(',')} extra=${extra.join(',')}`)
  if (actual.some(({ status }) => status === 'skipped')) throw new Error('Unexpected skipped cases in final ledger')
}

export async function writeLedgerAtomic(path: string, ledger: CaseLedger): Promise<void> {
  assertUniqueIds(ledger.cases); await mkdir(dirname(path), { recursive: true })
  const temp = `${path}.${process.pid}.tmp`; await writeFile(temp, `${JSON.stringify(ledger, null, 2)}\n`, { mode: 0o600 }); await rename(temp, path)
}

export async function readLedger(path: string): Promise<CaseLedger> {
  return JSON.parse(await readFile(path, 'utf8')) as CaseLedger
}
