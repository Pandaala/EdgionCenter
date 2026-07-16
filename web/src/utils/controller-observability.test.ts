import { describe, expect, it } from 'vitest'
import type { K8sResource } from '@/api/types'
import {
  buildConsistencyRows,
  isCertificateExpiring,
  resourceFingerprint,
  resourceIssues,
  type ControllerResourceSnapshot,
} from './controller-observability'

function route(name: string, backend: string, status?: unknown): K8sResource {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1', kind: 'HTTPRoute',
    metadata: { name, namespace: 'demo', resourceVersion: Math.random().toString() },
    spec: { rules: [{ backendRefs: [{ name: backend, port: 80 }] }] }, status,
  }
}

describe('controller observability', () => {
  it('ignores server metadata and status in the config fingerprint', () => {
    const a = route('web', 'svc', { conditions: [{ type: 'Accepted', status: 'True' }] })
    const b = { ...route('web', 'svc', { conditions: [{ type: 'Accepted', status: 'False' }] }), metadata: { ...a.metadata, resourceVersion: '999' } }
    expect(resourceFingerprint(a)).toBe(resourceFingerprint(b))
  })
  it('preserves nested operator fields named status and generation', () => {
    const a = route('web', 'svc'); const b = route('web', 'svc')
    a.spec.rules[0].filters = [{ type: 'ExtensionRef', extensionRef: { name: 'p', status: 'blue', generation: 1 } }]
    b.spec.rules[0].filters = [{ type: 'ExtensionRef', extensionRef: { name: 'p', status: 'green', generation: 2 } }]
    expect(resourceFingerprint(a, 'httproute')).not.toBe(resourceFingerprint(b, 'httproute'))
  })

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

  it('fingerprints EndpointSlice operator fields and normalizes only semantic sets', () => {
    const slice = (endpoints: unknown[], ports: unknown[]) => ({
      apiVersion: 'discovery.k8s.io/v1', kind: 'EndpointSlice', metadata: { name: 's', namespace: 'n' },
      addressType: 'IPv4', endpoints, ports,
    } as unknown as K8sResource)
    const a = slice([{ addresses: ['10.0.0.2', '10.0.0.1'] }, { addresses: ['10.0.0.3'] }], [{ port: 81 }, { port: 80 }])
    const b = slice([{ addresses: ['10.0.0.3'] }, { addresses: ['10.0.0.1', '10.0.0.2'] }], [{ port: 80 }, { port: 81 }])
    expect(resourceFingerprint(a, 'endpointslice')).toBe(resourceFingerprint(b, 'endpointslice'))
    ;(b as any).addressType = 'IPv6'
    expect(resourceFingerprint(a, 'endpointslice')).not.toBe(resourceFingerprint(b, 'endpointslice'))
    const orderedA = route('web', 'a'); const orderedB = route('web', 'b')
    orderedA.spec.rules = [{ backendRefs: [{ name: 'a' }, { name: 'b' }] }]
    orderedB.spec.rules = [{ backendRefs: [{ name: 'b' }, { name: 'a' }] }]
    expect(resourceFingerprint(orderedA, 'httproute')).not.toBe(resourceFingerprint(orderedB, 'httproute'))
  })

  it('reports missing and divergent resources across controllers', () => {
    const snapshots: ControllerResourceSnapshot[] = [
      { controllerId: 'a', cluster: 'east', resources: { httproute: [route('web', 'svc-a'), route('only-a', 'svc')] }, errors: [] },
      { controllerId: 'b', cluster: 'west', resources: { httproute: [route('web', 'svc-b')] }, errors: [] },
    ]
    const rows = buildConsistencyRows(snapshots)
    expect(rows.find((row) => row.name === 'web')?.consistent).toBe(false)
    expect(rows.find((row) => row.name === 'only-a')?.controllers.b.present).toBe(false)
  })

  it('does not report drift when a Controller/kind snapshot is unavailable', () => {
    const snapshots: ControllerResourceSnapshot[] = [
      { controllerId: 'a', cluster: 'east', resources: { httproute: [route('web', 'svc')] }, errors: [] },
      { controllerId: 'b', cluster: 'west', resources: {}, errors: ['httproute'] },
    ]
    const row = buildConsistencyRows(snapshots)[0]
    expect(row.consistent).toBeNull()
    expect(row.controllers.b.available).toBe(false)
  })

  it('uses a bounded 30-day certificate window', () => {
    const now = Date.parse('2026-07-15T00:00:00Z')
    expect(isCertificateExpiring('2026-07-20T00:00:00Z', now)).toBe(true)
    expect(isCertificateExpiring('2026-09-20T00:00:00Z', now)).toBe(false)
    expect(isCertificateExpiring('2026-07-01T00:00:00Z', now)).toBe(false)
  })
})
