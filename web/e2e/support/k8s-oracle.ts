import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { kubectlArgs } from '../scripts/ledger.ts'
const execFileAsync = promisify(execFile)

export interface K8sIdentity { resource: string; name: string; namespace?: string }

export async function kubectlJson(identity: K8sIdentity): Promise<Record<string, unknown>> {
  const args = ['get', identity.resource, identity.name, '-o', 'json']
  if (identity.namespace) args.push('-n', identity.namespace)
  const { stdout } = await execFileAsync('kubectl', kubectlArgs(args), { maxBuffer: 8 * 1024 * 1024 })
  return JSON.parse(stdout) as Record<string, unknown>
}

export async function assertIdentity(identity: K8sIdentity, expectedUid: string, runId: string): Promise<void> {
  const value = await kubectlJson(identity) as { metadata?: { uid?: string; labels?: Record<string, string> } }
  if (value.metadata?.uid !== expectedUid) throw new Error(`UID mismatch for ${identity.resource}/${identity.name}`)
  if (value.metadata?.labels?.['edgion.io/e2e-run'] !== runId) throw new Error(`Run label mismatch for ${identity.resource}/${identity.name}`)
}
