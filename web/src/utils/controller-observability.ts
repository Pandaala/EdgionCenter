import type { K8sResource, ResourceKind } from '@/api/types'
import { collectResourceConditions } from '@/components/resource/ResourceConditions'
import { buildMutationDocument } from '@/utils/resource-document'

export type ResourceIssue = 'unresolved' | 'rejected' | 'conflict'

function stable(value: unknown, path: string[] = [], unorderedPaths = new Set<string>()): unknown {
  if (Array.isArray(value)) {
    const items = value.map((item) => stable(item, [...path, '*'], unorderedPaths))
    const unordered = unorderedPaths.has(path.join('.'))
    return unordered ? items.sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))) : items
  }
  if (typeof value !== 'object' || value === null) return value
  return Object.fromEntries(Object.entries(value as Record<string, unknown>)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, child]) => [key, stable(child, [...path, key], unorderedPaths)]))
}

export function resourceFingerprint(resource: K8sResource, resourceKind?: ResourceKind): string {
  let operatorDocument: Record<string, unknown> = resource as unknown as Record<string, unknown>
  if (resourceKind) {
    try {
      operatorDocument = buildMutationDocument(resource, { mode: 'update', resourceKind })
    } catch {
      // Older Controllers can return a compatibility version. The fallback is
      // still status/server-metadata free and keeps the comparison available.
    }
  }
  const operatorFields = { ...operatorDocument }
  const rawMetadata = operatorFields.metadata
  delete operatorFields.apiVersion
  delete operatorFields.kind
  delete operatorFields.metadata
  delete operatorFields.status
  const metadata = rawMetadata as Record<string, unknown> | undefined
  const unorderedPaths = resourceKind === 'endpointslice'
    ? new Set(['endpoints', 'ports', 'endpoints.*.addresses'])
    : new Set<string>()
  return JSON.stringify(stable({
    metadata: {
      labels: metadata?.labels ?? resource.metadata.labels,
      annotations: metadata?.annotations ?? resource.metadata.annotations,
    },
    ...operatorFields,
  }, [], unorderedPaths))
}

export function resourceIssues(resource: K8sResource): ResourceIssue[] {
  const issues = new Set<ResourceIssue>()
  collectResourceConditions(resource.status).forEach(({ condition }) => {
    const text = `${condition.type} ${condition.reason ?? ''} ${condition.message ?? ''}`
    const reason = condition.reason ?? ''
    const conflictReason = /conflict/i.test(reason) && !/^NoConflict/i.test(reason) && !/Resolved/i.test(reason)
    if (condition.status === 'False' && condition.type === 'ResolvedRefs') issues.add('unresolved')
    if (condition.status === 'False' && condition.type === 'Accepted') issues.add('rejected')
    if (condition.status === 'False' && /unresolved|not.?found|refnotpermitted|invalid.?ref/i.test(text)) issues.add('unresolved')
    if (condition.status === 'True' && condition.type !== 'NoConflicts'
      && (/^(Conflict|Conflicted|Conflicts)$/i.test(condition.type) || conflictReason)) issues.add('conflict')
  })
  return [...issues]
}

export function isCertificateExpiring(value: string | undefined, now = Date.now(), withinDays = 30): boolean {
  if (!value) return false
  const expiresAt = Date.parse(value)
  return Number.isFinite(expiresAt) && expiresAt >= now && expiresAt - now <= withinDays * 86_400_000
}
