import { createHash } from 'node:crypto'
import { readFile, rename, unlink, writeFile } from 'node:fs/promises'
import { resolve, sep } from 'node:path'
import * as yaml from 'js-yaml'
import { requireRun } from './ledger.ts'

interface FileEntry { path: string; sha256: string }
interface FileLedger { schemaVersion: 1; runId: string; root: string; retained?: boolean; files: FileEntry[] }
const { runId, artifactDir } = requireRun(); const path = resolve(artifactDir, 'standalone-files-ledger.json')
const ledger = JSON.parse(await readFile(path, 'utf8')) as FileLedger
if (ledger.runId !== runId || resolve(ledger.root) !== artifactDir) throw new Error('Standalone cleanup run/root mismatch')
const retain = process.env.E2E_RETAIN_ENV === '1'
let changed = 0
for (const item of ledger.files) {
  if (!resolve(item.path).startsWith(`${artifactDir}${sep}`)) throw new Error(`Path escapes artifact root: ${item.path}`)
  const content = await readFile(item.path)
  if (retain) {
    const document = yaml.load(content.toString()) as { metadata?: { labels?: Record<string, string> } }
    if (document?.metadata?.labels?.['edgion.io/e2e-run'] !== runId) throw new Error(`Fixture run label mismatch: ${item.path}`)
  }
  const digest = createHash('sha256').update(content).digest('hex')
  if (digest !== item.sha256) {
    if (!retain) throw new Error(`Fixture changed since seed: ${item.path}`)
    changed += 1
  }
}
if (retain) {
  ledger.retained = true; const temp = `${path}.${process.pid}.tmp`; await writeFile(temp, `${JSON.stringify(ledger, null, 2)}\n`, { mode: 0o600 }); await rename(temp, path)
  process.stdout.write(`retained ${ledger.files.length} verified standalone files (${changed} changed; original deletion hashes preserved)\n`)
} else {
  for (const item of ledger.files) await unlink(item.path)
  process.stdout.write(`deleted ${ledger.files.length} exact standalone files\n`)
}
