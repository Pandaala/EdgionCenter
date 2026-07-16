import { createHash } from 'node:crypto'
import { readFile, rename, unlink, writeFile } from 'node:fs/promises'
import { resolve, sep } from 'node:path'
import { requireRun } from './ledger.ts'

interface FileEntry { path: string; sha256: string }
interface FileLedger { schemaVersion: 1; runId: string; root: string; retained?: boolean; files: FileEntry[] }
const { runId, artifactDir } = requireRun(); const path = resolve(artifactDir, 'standalone-files-ledger.json')
const ledger = JSON.parse(await readFile(path, 'utf8')) as FileLedger
if (ledger.runId !== runId || resolve(ledger.root) !== artifactDir) throw new Error('Standalone cleanup run/root mismatch')
for (const item of ledger.files) {
  if (!resolve(item.path).startsWith(`${artifactDir}${sep}`)) throw new Error(`Path escapes artifact root: ${item.path}`)
  const digest = createHash('sha256').update(await readFile(item.path)).digest('hex'); if (digest !== item.sha256) throw new Error(`Fixture changed since seed: ${item.path}`)
}
if (process.env.E2E_RETAIN_ENV === '1') {
  ledger.retained = true; const temp = `${path}.${process.pid}.tmp`; await writeFile(temp, `${JSON.stringify(ledger, null, 2)}\n`, { mode: 0o600 }); await rename(temp, path)
  process.stdout.write(`retained ${ledger.files.length} verified standalone files\n`)
} else {
  for (const item of ledger.files) await unlink(item.path)
  process.stdout.write(`deleted ${ledger.files.length} exact standalone files\n`)
}
