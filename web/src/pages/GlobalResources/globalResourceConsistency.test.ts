import { describe, expect, it } from 'vitest'
import type { GlobalResourceComparisonGroup } from '@/api/globalResources'
import {
  getGlobalResourceConsistency,
  normalizeGlobalResource,
} from './globalResourceConsistency'

function groupWith(...objects: GlobalResourceComparisonGroup['members'][number]['object'][]) {
  return {
    key: { kind: 'EdgionConfigData', namespace: 'edgion-system', name: 'public-route' },
    members: objects.map((object, index) => ({
      cluster: `cluster-${index}`,
      controllerId: `controller-${index}`,
      object,
    })),
  }
}

describe('global resource consistency', () => {
  it('deep-clones, sorts object keys, and removes only top-level Kubernetes-owned fields', () => {
    const source = {
      status: { accepted: true },
      spec: {
        rules: [{ backendRefs: [{ name: 'backend-b' }, { name: 'backend-a' }] }],
        nested: {
          metadata: {
            name: 'kept',
            resourceVersion: '17',
            uid: 'uid-1',
            generation: 4,
            creationTimestamp: 'now',
            managedFields: [{ manager: 'controller' }],
          },
          status: { ignored: true },
        },
      },
      metadata: { namespace: 'edgion-system', name: 'public-route' },
      apiVersion: 'gateway.networking.k8s.io/v1',
    }

    const normalized = normalizeGlobalResource(source)

    expect(normalized).toEqual({
      apiVersion: 'gateway.networking.k8s.io/v1',
      metadata: { name: 'public-route', namespace: 'edgion-system' },
      spec: {
        nested: {
          metadata: {
            creationTimestamp: 'now',
            generation: 4,
            managedFields: [{ manager: 'controller' }],
            name: 'kept',
            resourceVersion: '17',
            uid: 'uid-1',
          },
          status: { ignored: true },
        },
        rules: [{ backendRefs: [{ name: 'backend-b' }, { name: 'backend-a' }] }],
      },
    })
    expect(source.status).toEqual({ accepted: true })
    expect(source.spec.rules[0].backendRefs.map(({ name }) => name)).toEqual([
      'backend-b',
      'backend-a',
    ])
  })

  it('treats metadata and status differences as consistent', () => {
    const first = {
      metadata: { name: 'route', namespace: 'edgion-system', resourceVersion: '1', uid: 'a' },
      spec: { hostnames: ['example.com'] },
      status: { parents: ['a'] },
    }
    const second = {
      spec: { hostnames: ['example.com'] },
      status: { parents: ['b'] },
      metadata: { uid: 'b', resourceVersion: '2', namespace: 'edgion-system', name: 'route' },
    }

    expect(getGlobalResourceConsistency(groupWith(first, second))).toBe('consistent')
  })

  it('preserves array order and reports meaningful differences', () => {
    const first = { spec: { hostnames: ['a.example.com', 'b.example.com'] } }
    const second = { spec: { hostnames: ['b.example.com', 'a.example.com'] } }

    expect(getGlobalResourceConsistency(groupWith(first, second))).toBe('inconsistent')
  })

  it('preserves nested business status fields when comparing resources', () => {
    const first = { spec: { status: 'enabled' } }
    const second = { spec: { status: 'disabled' } }

    expect(getGlobalResourceConsistency(groupWith(first, second))).toBe('inconsistent')
  })

  it('classifies fewer than two members as single', () => {
    expect(getGlobalResourceConsistency(groupWith({ spec: {} }))).toBe('single')
    expect(getGlobalResourceConsistency(groupWith())).toBe('single')
  })
})
