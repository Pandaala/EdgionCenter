import { createHash } from 'node:crypto'
import { execFile } from 'node:child_process'
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { promisify } from 'node:util'
const cleanupKindMap = JSON.parse(readFileSync(new URL('../cleanup-kind-map.json', import.meta.url), 'utf8')) as Record<string, string>

const execFileAsync = promisify(execFile)
export interface CleanupObject {
  apiVersion: string; kind: string; resource: string; scope: 'Namespaced' | 'Cluster'; namespace?: string
  name: string; runLabel: string; uid: string; phase: 'runtime' | 'seed' | 'auth'
}
export interface CleanupLedger { schemaVersion: 1; runId: string; context: string; createdAt: string; retained?: boolean; objects: CleanupObject[] }
const runtimeKinds: Record<string, string> = {
  Namespace: 'namespaces', Deployment: 'deployments.apps', Service: 'services', ServiceAccount: 'serviceaccounts',
  Role: 'roles.rbac.authorization.k8s.io', RoleBinding: 'rolebindings.rbac.authorization.k8s.io',
  ClusterRole: 'clusterroles.rbac.authorization.k8s.io', ClusterRoleBinding: 'clusterrolebindings.rbac.authorization.k8s.io',
}

export function requireRun(): { runId: string; artifactDir: string; prefix: string } {
  const runId = process.env.E2E_RUN_ID
  if (!runId) throw new Error('E2E_RUN_ID is required; run via e2e/scripts/run.sh')
  if (process.env.E2E_ALLOW_MUTATION !== '1') throw new Error('Refusing mutation without E2E_ALLOW_MUTATION=1')
  const artifactDir = resolve(process.env.E2E_ARTIFACT_DIR ?? `test-results/${runId}`)
  return { runId, artifactDir, prefix: `eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}` }
}
export function substitutions(runId: string, prefix: string): Record<string, string> {
  return { __RUN_ID__: runId, __PREFIX__: prefix, __NS_A__: `${prefix}-a`, __NS_B__: `${prefix}-b`, __NS_DENIED__: `${prefix}-denied` }
}
export function render(source: string, values: Record<string, string>): string {
  return Object.entries(values).reduce((value, [key, replacement]) => value.replaceAll(key, replacement), source)
}
export function resourceForKind(kind: string): string {
  const value = (cleanupKindMap as Record<string, string>)[kind] ?? runtimeKinds[kind]
  if (!value) throw new Error(`Unknown cleanup resource plural for ${kind}`)
  return value
}
export function kubeContext(): string {
  const context = process.env.E2E_KUBE_CONTEXT
  if (context !== 'orbstack') throw new Error('E2E_KUBE_CONTEXT must be exactly "orbstack"')
  return context
}
export function kubectlArgs(args: string[]): string[] { return ['--context', kubeContext(), ...args] }
export async function currentContext(): Promise<string> {
  const context = kubeContext()
  const value = (await execFileAsync('kubectl', ['config', 'get-contexts', context, '-o', 'name'])).stdout.trim()
  if (value !== context) throw new Error(`Kubernetes context is unavailable: ${context}`)
  return context
}
export async function writeCleanupLedger(path: string, ledger: CleanupLedger): Promise<void> {
  await mkdir(dirname(path), { recursive: true }); const temp = `${path}.${process.pid}.tmp`
  await writeFile(temp, `${JSON.stringify(ledger, null, 2)}\n`, { mode: 0o600 }); await rename(temp, path)
}
export async function readCleanupLedger(path: string): Promise<CleanupLedger> {
  const value = JSON.parse(await readFile(path, 'utf8')) as Partial<CleanupLedger>
  if (value.schemaVersion !== 1 || typeof value.runId !== 'string' || !value.runId || typeof value.context !== 'string' || !value.context || typeof value.createdAt !== 'string' || !Array.isArray(value.objects) || value.objects.length === 0) throw new Error('Cleanup ledger does not satisfy the required envelope')
  const identities = new Set<string>()
  for (const item of value.objects as CleanupObject[]) {
    if (!item || !item.apiVersion || !item.kind || !item.resource || !item.name || !item.runLabel || !item.uid || !['runtime', 'seed', 'auth'].includes(item.phase)) throw new Error('Cleanup ledger contains an invalid object entry')
    if (!['Namespaced', 'Cluster'].includes(item.scope) || (item.scope === 'Namespaced') !== Boolean(item.namespace)) throw new Error(`Cleanup ledger scope/namespace mismatch for ${item.kind}/${item.name}`)
    const identity = `${item.apiVersion}/${item.kind}/${item.namespace ?? '_cluster'}/${item.name}`
    if (identities.has(identity)) throw new Error(`Cleanup ledger contains duplicate identity ${identity}`)
    identities.add(identity)
  }
  return value as CleanupLedger
}
