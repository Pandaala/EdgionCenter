import { spawnSync } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import * as yaml from 'js-yaml'
import inventory from '../fixture-inventory.json'
import { currentContext, kubectlArgs, readCleanupLedger, render, requireRun, resourceForKind, substitutions, writeCleanupLedger, type CleanupLedger, type CleanupObject } from './ledger.ts'

async function create(document: Record<string, unknown>, dryRun = false): Promise<Record<string, any>> {
  const source = yaml.dump(document, { lineWidth: -1 })
  const args = ['create', '-f', '-', '-o', 'json']
  if (dryRun) args.push('--dry-run=server')
  const result = spawnSync('kubectl', kubectlArgs(args), { input: source, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 })
  if (result.status !== 0) throw new Error(`kubectl ${dryRun ? 'server dry-run' : 'create'} refused${dryRun ? ' fixture schema' : ' (fixtures must be absent before the run)'}: ${result.stderr}`)
  return JSON.parse(result.stdout) as Record<string, any>
}

function patchStatus(document: Record<string, any>): void {
  if (!document.status) return
  const resource = resourceForKind(document.kind)
  const args = ['patch', resource, document.metadata.name]
  if (document.metadata.namespace) args.push('-n', document.metadata.namespace)
  args.push('--subresource=status', '--type=merge', '-p', JSON.stringify({ status: document.status }))
  const result = spawnSync('kubectl', kubectlArgs(args), { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 })
  if (result.status !== 0) throw new Error(`kubectl status patch refused for ${document.kind}/${document.metadata.name}: ${result.stderr}`)
}

const { runId, artifactDir, prefix } = requireRun()
const indentPem = (value: string) => value.trimEnd().replaceAll('\n', '\n    ')
const values = {
  ...substitutions(runId, prefix),
  __FIXTURE_TLS_CERT__: indentPem(await readFile(resolve(artifactDir, 'tls/server.crt'), 'utf8')),
  __FIXTURE_TLS_KEY__: indentPem(await readFile(resolve(artifactDir, 'tls/server.key'), 'utf8')),
}
const ledgerPath = resolve(artifactDir, 'cleanup-ledger.json'); const context = await currentContext()
const missingOnly = process.env.E2E_SEED_MISSING_ONLY === '1'
let ledger: CleanupLedger
try { ledger = await readCleanupLedger(ledgerPath) } catch (error) {
  if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
  ledger = { schemaVersion: 1, runId, context, createdAt: new Date().toISOString(), objects: [] }
}
if (ledger.runId !== runId || ledger.context !== context) throw new Error('Seed refused: existing ledger run/context mismatch')
const allowed = new Set<string>()
const declaredIdentity = (item: { kind: string; name: string; namespace?: string }) => `${item.kind}/${item.namespace ? render(item.namespace, values) : '_cluster'}/${render(item.name, values)}`
for (const item of inventory.catalogResources) allowed.add(declaredIdentity(item))
for (const item of [...inventory.states, ...inventory.supportResources]) allowed.add(declaredIdentity(item))
try {
  const fixtureFiles = [...new Set([...inventory.catalogResources, ...inventory.states, ...inventory.supportResources].map(({ file }) => file))]
  const documents: Array<Record<string, any>> = []
  for (const file of fixtureFiles) {
    const source = render(await readFile(resolve('e2e/fixtures', file), 'utf8'), values)
    yaml.loadAll(source, (value) => { if (value) documents.push(value as Record<string, any>) })
  }
  // Dependency Secrets must exist before resources whose controllers resolve
  // them immediately after the create watch event.
  documents.sort((left, right) => Number(right.kind === 'Secret') - Number(left.kind === 'Secret'))
  const ledgerIdentities = new Set(ledger.objects.map((item) => `${item.kind}/${item.namespace ?? '_cluster'}/${item.name}`))
  const selectedDocuments = missingOnly
    ? documents.filter((document) => !ledgerIdentities.has(`${document.kind}/${document.metadata?.namespace ?? '_cluster'}/${document.metadata?.name}`))
    : documents
  for (const document of selectedDocuments) {
    const identity = `${document.kind}/${document.metadata?.namespace ?? '_cluster'}/${document.metadata?.name}`; if (!allowed.has(identity)) throw new Error(`Fixture not declared in static inventory: ${identity}`)
    if (document.metadata?.labels?.['edgion.io/e2e-run'] !== runId) throw new Error(`Fixture missing exact run label: ${identity}`)
    await create(document, true)
  }
  for (const document of selectedDocuments) {
      const identity = `${document.kind}/${document.metadata?.namespace ?? '_cluster'}/${document.metadata?.name}`; if (!allowed.has(identity)) throw new Error(`Fixture not declared in static inventory: ${identity}`)
      if (document.metadata?.labels?.['edgion.io/e2e-run'] !== runId) throw new Error(`Fixture missing exact run label: ${identity}`)
      const applied = await create(document)
      patchStatus(document)
      const scope = applied.metadata.namespace ? 'Namespaced' : 'Cluster'
      const entry: CleanupObject = { apiVersion: applied.apiVersion, kind: applied.kind, resource: resourceForKind(applied.kind), scope, name: applied.metadata.name, runLabel: runId, uid: applied.metadata.uid, phase: 'seed' }
      if (scope === 'Namespaced') entry.namespace = applied.metadata.namespace
      ledger.objects.push(entry); await writeCleanupLedger(ledgerPath, ledger)
  }
  process.stdout.write(`${missingOnly ? `added ${selectedDocuments.length} missing fixtures; ` : ''}seeded ${ledger.objects.length} exact objects; ledger=${ledgerPath}\n`)
} catch (error) {
  await writeCleanupLedger(ledgerPath, ledger)
  process.stderr.write(`seed failed after ${ledger.objects.length} objects; recover with E2E_ALLOW_MUTATION=1 tsx e2e/scripts/cleanup.ts ${ledgerPath}\n`)
  throw error
}
