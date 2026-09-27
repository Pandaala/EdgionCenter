import { describe, expect, it } from 'vitest'
import type { K8sResource } from '@/api/types'
import { resourceIssues } from './controller-observability'

function route(name: string, backend: string, status?: unknown): K8sResource {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1', kind: 'HTTPRoute',
    metadata: { name, namespace: 'demo', resourceVersion: Math.random().toString() },
    spec: { rules: [{ backendRefs: [{ name: backend, port: 80 }] }] }, status,
  }
}

describe('controller observability', () => {
  it('detects rejected, unresolved, and conflict diagnostics', () => {
    const issues = resourceIssues(route('web', 'svc', {
      conditions: [
        { type: 'Accepted', status: 'False' },
        { type: 'ResolvedRefs', status: 'False', reason: 'BackendNotFound' },
        { type: 'Conflicted', status: 'True', reason: 'FileConflict' },
      ],
    }))
    expect(issues).toEqual(expect.arrayContaining(['rejected', 'unresolved', 'conflict']))
  })
  it('does not treat NoConflicts=False or Ready=False as conflict/rejected', () => {
    expect(resourceIssues(route('web', 'svc', { conditions: [
      { type: 'NoConflicts', status: 'False', reason: 'ConflictsFound' },
      { type: 'Ready', status: 'False', reason: 'Pending' },
    ] }))).toEqual([])
  })
  it('honors condition truth before text-shaped conflict or unresolved reasons', () => {
    expect(resourceIssues(route('web', 'svc', { conditions: [
      { type: 'Conflicted', status: 'False', reason: 'ConflictResolved', message: 'no conflict remains' },
      { type: 'ResolvedRefs', status: 'True', reason: 'BackendNotFoundPreviously', message: 'resolved' },
      { type: 'Healthy', status: 'True', reason: 'NoConflict' },
    ] }))).toEqual([])
    expect(resourceIssues(route('web', 'svc', { conditions: [
      { type: 'Conflicted', status: 'True', reason: 'DuplicateConfig' },
      { type: 'ResolvedRefs', status: 'False', reason: 'BackendNotFound' },
    ] }))).toEqual(expect.arrayContaining(['conflict', 'unresolved']))
  })
})


it('keeps current writer diagnostics while excluding obsolete rejection/conflict', () => {
  const value = route('web', 'svc', { controllers: [
    { controllerName: 'old', status: { conditions: [
      { type: 'Accepted', status: 'False', observedGeneration: 2 },
      { type: 'Conflicted', status: 'True', observedGeneration: 2 },
    ] } },
    { controllerName: 'current', status: { conditions: [
      { type: 'Accepted', status: 'True', observedGeneration: 3 },
      { type: 'PartiallyInvalid', status: 'True', observedGeneration: 3 },
      { type: 'ResolvedRefs', status: 'False', observedGeneration: 3 },
    ] } },
  ] })
  value.metadata.generation = 3
  expect(resourceIssues(value)).toEqual(['stale', 'partiallyInvalid', 'unresolved'])
})

it('does not infer partial invalidity from False or Unknown', () => {
  for (const status of ['False', 'Unknown']) {
    expect(resourceIssues(route('web', 'svc', { conditions: [
      { type: 'PartiallyInvalid', status },
    ] }))).toEqual([])
  }
})
