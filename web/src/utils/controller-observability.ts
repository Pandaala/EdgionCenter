import type { K8sResource, ResourceKind } from '@/api/types'
import { collectResourceConditions } from '@/components/resource/ResourceConditions'
import { buildMutationDocument } from '@/utils/resource-document'

export interface ControllerResourceSnapshot {
  controllerId: string
  cluster: string
  resources: Partial<Record<ResourceKind, K8sResource[]>>
  errors: ResourceKind[]
  fileConflicts?: Array<{ kind: string; key: string; winner: string; losers: string[] }>
  diagnosticsAvailable?: boolean
}

export type ResourceIssue = 'unresolved' | 'rejected' | 'conflict'

export interface ResourceObservation {
  controllerId: string
  cluster: string
  kind: ResourceKind
  namespace?: string
  name: string
  fingerprint: string
  issues: ResourceIssue[]
  conditionStates: string[]
  certificateNotAfter?: string
}

export interface ConsistencyRow {
  key: string
  kind: ResourceKind
  namespace?: string
  name: string
  /** null means at least one Controller/kind snapshot was unavailable. */
  consistent: boolean | null
  controllers: Record<string, { available: boolean; present: boolean; fingerprint?: string; issues: ResourceIssue[]; conditionStates: string[] }>
}

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

function certificateExpiry(resource: K8sResource): string | undefined {
  const status = resource.status as Record<string, unknown> | undefined
  const value = status?.certificateNotAfter ?? status?.notAfter
  return typeof value === 'string' ? value : undefined
}

export function observations(snapshot: ControllerResourceSnapshot): ResourceObservation[] {
  return Object.entries(snapshot.resources).flatMap(([kind, resources]) => (
    (resources ?? []).map((resource) => ({
      controllerId: snapshot.controllerId,
      cluster: snapshot.cluster,
      kind: kind as ResourceKind,
      namespace: resource.metadata.namespace,
      name: resource.metadata.name,
      fingerprint: resourceFingerprint(resource, kind as ResourceKind),
      issues: resourceIssues(resource),
      conditionStates: collectResourceConditions(resource.status).map(({ context, condition }) => `${context}: ${condition.type}=${condition.status}`),
      certificateNotAfter: certificateExpiry(resource),
    }))
  ))
}

export function buildConsistencyRows(snapshots: ControllerResourceSnapshot[]): ConsistencyRow[] {
  const byKey = new Map<string, ResourceObservation[]>()
  snapshots.flatMap(observations).forEach((observation) => {
    const key = `${observation.kind}/${observation.namespace ?? '_cluster'}/${observation.name}`
    byKey.set(key, [...(byKey.get(key) ?? []), observation])
  })
  return [...byKey.entries()].map(([key, items]) => {
    const first = items[0]
    const states = Object.fromEntries(snapshots.map((snapshot) => {
      const item = items.find((candidate) => candidate.controllerId === snapshot.controllerId)
      return [snapshot.controllerId, {
        available: !snapshot.errors.includes(first.kind),
        present: Boolean(item),
        fingerprint: item?.fingerprint,
        issues: item?.issues ?? [],
        conditionStates: item?.conditionStates ?? [],
      }]
    }))
    const fingerprints = new Set(items.map((item) => item.fingerprint))
    const unavailable = snapshots.some((snapshot) => snapshot.errors.includes(first.kind))
    return {
      key, kind: first.kind, namespace: first.namespace, name: first.name,
      consistent: unavailable ? null : items.length === snapshots.length && fingerprints.size === 1,
      controllers: states,
    }
  }).sort((a, b) => a.key.localeCompare(b.key))
}

export function isCertificateExpiring(value: string | undefined, now = Date.now(), withinDays = 30): boolean {
  if (!value) return false
  const expiresAt = Date.parse(value)
  return Number.isFinite(expiresAt) && expiresAt >= now && expiresAt - now <= withinDays * 86_400_000
}
