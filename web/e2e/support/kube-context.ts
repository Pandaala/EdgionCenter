import { createHash } from 'node:crypto'

export function validateKubeContext(context: string | undefined, runId: string | undefined): string {
  if (context === 'orbstack') return context
  if (runId) {
    const expected = `kind-eruie2e-${createHash('sha256').update(runId).digest('hex').slice(0, 8)}`
    if (context === expected) return context
  }
  throw new Error('E2E_KUBE_CONTEXT must be orbstack or the kind-eruie2e-<run hash> context for E2E_RUN_ID')
}
