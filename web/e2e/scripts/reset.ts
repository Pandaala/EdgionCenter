import { spawnSync } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import * as yaml from 'js-yaml'
import inventory from '../fixture-inventory.json'
import { currentContext, kubectlArgs, readCleanupLedger, render, requireRun, substitutions } from './ledger.ts'
import { buildCasReplacement } from '../support/reset-document.ts'

const { runId, artifactDir, prefix } = requireRun()
const ledger = await readCleanupLedger(resolve(artifactDir, 'cleanup-ledger.json'))
if (ledger.context !== await currentContext() || ledger.runId !== runId) throw new Error('Reset context/run mismatch')
const identity = (kind: string, namespace: string | undefined, name: string) => `${kind}/${namespace ?? '_cluster'}/${name}`
const owned = new Map(ledger.objects.map((item) => [identity(item.kind, item.namespace, item.name), item]))
for (const file of [...new Set([...inventory.catalogResources, ...inventory.states, ...inventory.supportResources].map(({ file }) => file))]) {
  const documents: unknown[] = []; yaml.loadAll(render(await readFile(resolve('e2e/fixtures', file), 'utf8'), substitutions(runId, prefix)), (value) => documents.push(value))
  for (const document of documents as Array<Record<string, any>>) {
    const expected = owned.get(identity(document.kind, document.metadata.namespace, document.metadata.name)); if (!expected) throw new Error(`Reset refused for unledgered object ${document.kind}/${document.metadata.name}`)
    const getArgs = ['get', expected.resource, expected.name, '-o', 'json']
    if (expected.namespace) getArgs.push('-n', expected.namespace)
    const currentResult = spawnSync('kubectl', kubectlArgs(getArgs), { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 })
    if (currentResult.status !== 0) throw new Error(`Reset refused because owned object is missing: ${expected.resource}/${expected.name}`)
    const current = JSON.parse(currentResult.stdout) as { metadata?: { uid?: string; resourceVersion?: string; labels?: Record<string, string> } }
    if (current.metadata?.uid !== expected.uid || current.metadata?.labels?.['edgion.io/e2e-run'] !== runId) throw new Error(`Reset ownership mismatch: ${expected.resource}/${expected.name}`)
    if (!current.metadata.resourceVersion) throw new Error(`Reset refused because resourceVersion is absent: ${expected.resource}/${expected.name}`)
    const replacement = buildCasReplacement(document, current as Record<string, any>)
    const result = spawnSync('kubectl', kubectlArgs(['replace', '-f', '-']), { input: yaml.dump(replacement), encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 })
    if (result.status !== 0) throw new Error(`kubectl CAS reset failed (concurrent changes are not overwritten): ${result.stderr}`)
    process.stdout.write(result.stdout)
  }
}
