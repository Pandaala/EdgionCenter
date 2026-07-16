import { spawn } from 'node:child_process'
import { resolve } from 'node:path'
import inventory from '../fixture-inventory.json'
import cleanupKindMap from '../cleanup-kind-map.json'
import { currentContext, kubectlArgs, readCleanupLedger, requireRun, substitutions, writeCleanupLedger, type CleanupObject } from './ledger.ts'

function apiPath(item: CleanupObject): string {
  const base = item.apiVersion === 'v1' ? '/api/v1' : `/apis/${item.apiVersion}`
  const namespace = item.scope === 'Namespaced' ? `/namespaces/${encodeURIComponent(item.namespace!)}` : ''
  return `${base}${namespace}/${item.resource.split('.')[0]}/${encodeURIComponent(item.name)}`
}
async function startProxy(): Promise<{ base: string; stop: () => Promise<void> }> {
  const child = spawn('kubectl', kubectlArgs(['proxy', '--port=0', '--accept-hosts=^127\\.0\\.0\\.1$']), { stdio: ['ignore', 'pipe', 'pipe'] })
  const port = await new Promise<string>((resolvePort, reject) => {
    const timeout = setTimeout(() => { child.kill('SIGTERM'); reject(new Error('kubectl proxy startup deadline exceeded')) }, 10_000)
    const inspect = (chunk: Buffer) => { const match = chunk.toString().match(/127\.0\.0\.1:(\d+)/); if (match) { clearTimeout(timeout); resolvePort(match[1]) } }
    child.stdout.on('data', inspect); child.stderr.on('data', inspect); child.once('exit', (code) => reject(new Error(`kubectl proxy exited early: ${code}`)))
  })
  return { base: `http://127.0.0.1:${port}`, stop: async () => { child.kill('SIGTERM'); await new Promise<void>((resolveExit) => child.once('exit', () => resolveExit())) } }
}
async function get(base: string, item: CleanupObject): Promise<Response> { return fetch(`${base}${apiPath(item)}`) }

const { runId, artifactDir, prefix } = requireRun(); const path = process.argv[2] ? resolve(process.argv[2]) : resolve(artifactDir, 'cleanup-ledger.json')
const ledger = await readCleanupLedger(path)
if (ledger.runId !== runId || ledger.context !== await currentContext()) throw new Error('Cleanup refused: run/context mismatch')
const values = substitutions(runId, prefix)
const seedIdentities = new Set([
  ...inventory.namespaces.map((name) => `Namespace/_cluster/${values[name as keyof typeof values] ?? name.replaceAll('__PREFIX__', prefix)}`),
  ...inventory.catalogResources.map((item) => `${item.kind}/${item.namespace ? Object.entries(values).reduce((name, [key, value]) => name.replaceAll(key, value), item.namespace) : '_cluster'}/${Object.entries(values).reduce((name, [key, value]) => name.replaceAll(key, value), item.name)}`),
  ...inventory.states.map((item) => `${item.kind}/${item.namespace ? Object.entries(values).reduce((name, [key, value]) => name.replaceAll(key, value), item.namespace) : '_cluster'}/${Object.entries(values).reduce((name, [key, value]) => name.replaceAll(key, value), item.name)}`),
  ...inventory.supportResources.map((item) => `${item.kind}/${item.namespace ? Object.entries(values).reduce((name, [key, value]) => name.replaceAll(key, value), item.namespace) : '_cluster'}/${Object.entries(values).reduce((name, [key, value]) => name.replaceAll(key, value), item.name)}`),
])
const runtimeResources = new Set(['namespaces', 'deployments.apps', 'services', 'serviceaccounts', 'roles.rbac.authorization.k8s.io', 'rolebindings.rbac.authorization.k8s.io', 'clusterroles.rbac.authorization.k8s.io', 'clusterrolebindings.rbac.authorization.k8s.io', 'configmaps', 'secrets'])
const catalogResources = new Set(Object.values(cleanupKindMap))
const runtimeIdentities = new Set(inventory.runtimeObjects.map(({ kind, name }) => `${kind}/${prefix}-${name}`))
for (const item of ledger.objects) {
  if (item.runLabel !== runId) throw new Error(`Cleanup refused: ledger label mismatch for ${item.kind}/${item.name}`)
  if (!catalogResources.has(item.resource) && !runtimeResources.has(item.resource)) throw new Error(`Cleanup refused: unknown resource ${item.resource}`)
  if (item.phase === 'seed' && !seedIdentities.has(`${item.kind}/${item.namespace ?? '_cluster'}/${item.name}`)) throw new Error(`Cleanup refused: object absent from static inventory ${item.kind}/${item.name}`)
  if (item.phase !== 'seed' && !runtimeIdentities.has(`${item.kind}/${item.name}`)) throw new Error(`Cleanup refused: runtime object absent from static inventory ${item.kind}/${item.name}`)
}
process.stdout.write(`${JSON.stringify(ledger.objects, null, 2)}\n`)
const proxy = await startProxy()
try {
  for (const item of ledger.objects) {
    const response = await get(proxy.base, item); if (!response.ok) throw new Error(`Owned object missing before cleanup: ${item.resource}/${item.name}`)
    const value = await response.json() as { metadata?: { uid?: string; labels?: Record<string, string> } }
    if (value.metadata?.uid !== item.uid || value.metadata?.labels?.['edgion.io/e2e-run'] !== runId) throw new Error(`Cleanup precondition mismatch: ${item.resource}/${item.name}`)
  }
  if (process.env.E2E_RETAIN_ENV === '1') {
    ledger.retained = true; await writeCleanupLedger(path, ledger); process.stdout.write(`retained ${ledger.objects.length} verified objects\n`)
  } else {
    const ordered = [...ledger.objects].sort((left, right) => Number(left.kind === 'Namespace') - Number(right.kind === 'Namespace'))
    for (const item of ordered) {
      const response = await fetch(`${proxy.base}${apiPath(item)}`, {
        method: 'DELETE', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ apiVersion: 'v1', kind: 'DeleteOptions', propagationPolicy: 'Foreground', preconditions: { uid: item.uid } }),
      })
      if (!response.ok && response.status !== 404) throw new Error(`Exact delete failed ${response.status}: ${item.resource}/${item.name} ${await response.text()}`)
      const deadline = Date.now() + 30_000
      while (Date.now() < deadline) { if ((await get(proxy.base, item)).status === 404) break; await new Promise((done) => setTimeout(done, 200)) }
      if ((await get(proxy.base, item)).status !== 404) throw new Error(`Deletion deadline exceeded: ${item.resource}/${item.name}`)
    }
    process.stdout.write(`deleted and proved absent: ${ordered.length} exact objects\n`)
  }
} finally { await proxy.stop() }
