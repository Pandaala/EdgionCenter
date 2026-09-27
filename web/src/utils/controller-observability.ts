import type { K8sResource } from '@/api/types'
import { collectResourceConditions, isConditionStale } from '@/components/resource/ResourceConditions'

export type ResourceIssue = 'unresolved' | 'rejected' | 'conflict' | 'stale' | 'partiallyInvalid'

export function resourceIssues(resource: K8sResource): ResourceIssue[] {
  const issues = new Set<ResourceIssue>()
  collectResourceConditions(resource.status).forEach(({ condition }) => {
    if (isConditionStale(condition, resource.metadata.generation)) {
      issues.add('stale')
      return
    }
    if (condition.type === 'PartiallyInvalid' && condition.status === 'True') issues.add('partiallyInvalid')
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
