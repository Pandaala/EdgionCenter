import { createHash } from 'node:crypto'
import { access, mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import * as yaml from 'js-yaml'
import inventory from '../fixture-inventory.json'
import { render, requireRun, substitutions } from './ledger.ts'

interface FileEntry { path: string; sha256: string; controller: 'A' | 'B' }
interface FileLedger { schemaVersion: 1; runId: string; root: string; retained?: boolean; files: FileEntry[] }
const { runId, artifactDir, prefix } = requireRun()
const indentPem = (value: string) => value.trimEnd().replaceAll('\n', '\n    ')
const values = {
  ...substitutions(runId, prefix),
  __FIXTURE_TLS_CERT__: indentPem(await readFile(resolve(artifactDir, 'tls/server.crt'), 'utf8')),
  __FIXTURE_TLS_KEY__: indentPem(await readFile(resolve(artifactDir, 'tls/server.key'), 'utf8')),
}
let files: FileEntry[] = []
const ledgerPath = resolve(artifactDir, 'standalone-files-ledger.json')
const reset = process.argv.includes('--reset')
if (reset) {
  const existing = JSON.parse(await readFile(ledgerPath, 'utf8')) as FileLedger
  if (existing.schemaVersion !== 1 || existing.runId !== runId || resolve(existing.root) !== artifactDir || !existing.files.length) throw new Error('Standalone reset ledger mismatch')
  files = existing.files.map((item) => ({ ...item }))
} else {
  try { await access(ledgerPath); throw new Error(`Standalone seed refused: ledger already exists at ${ledgerPath}`) } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
}
const visited = new Set<string>()
async function writeLedger(): Promise<void> {
  const ledger: FileLedger = { schemaVersion: 1, runId, root: artifactDir, files }
  const temp = `${ledgerPath}.${process.pid}.tmp`
  await writeFile(temp, `${JSON.stringify(ledger, null, 2)}\n`, { mode: 0o600 }); await rename(temp, ledgerPath)
}
for (const controller of ['A', 'B'] as const) {
  const directory = resolve(artifactDir, `controller-${controller.toLowerCase()}`, 'conf'); await mkdir(directory, { recursive: true })
  for (const fixture of [...new Set([...inventory.catalogResources, ...inventory.states, ...inventory.supportResources].map(({ file }) => file))]) {
    const documents: unknown[] = []; yaml.loadAll(render(await readFile(resolve('e2e/fixtures', fixture), 'utf8'), values), (value) => documents.push(value))
    for (const document of documents as Array<Record<string, any>>) {
      const namespace = document.metadata.namespace as string | undefined
      const subdirectory = resolve(directory, namespace ?? 'cluster'); await mkdir(subdirectory, { recursive: true })
      const fileName = namespace ? `${document.kind}_${namespace}_${document.metadata.name}.yaml` : `${document.kind}__${document.metadata.name}.yaml`
      const path = resolve(subdirectory, fileName)
      const source = yaml.dump(document, { lineWidth: -1 })
      if (reset) {
        const entry = files.find((item) => resolve(item.path) === path && item.controller === controller)
        if (!entry || visited.has(path)) throw new Error(`Standalone reset refused for unledgered or duplicate path ${path}`)
        const currentHash = createHash('sha256').update(await readFile(path)).digest('hex')
        if (currentHash !== entry.sha256) throw new Error(`Standalone reset ownership mismatch for ${path}`)
        await writeFile(path, source, { mode: 0o600 }); entry.sha256 = createHash('sha256').update(source).digest('hex'); visited.add(path)
      } else {
        try { await access(path); throw new Error(`Standalone seed refused to overwrite ${path}`) } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
        await writeFile(path, source, { mode: 0o600 })
        files.push({ path, sha256: createHash('sha256').update(source).digest('hex'), controller })
      }
      await writeLedger()
    }
  }
}
if (reset && visited.size !== files.length) throw new Error(`Standalone reset fixture set mismatch: visited ${visited.size}, ledger ${files.length}`)
process.stdout.write(`${reset ? 'reset' : 'seeded'} ${files.length} exact standalone fixture files; ledger=${ledgerPath}\n`)
