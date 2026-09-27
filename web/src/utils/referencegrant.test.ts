import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { normalize, toYaml, createEmpty, validateReferenceGrant } from './referencegrant'

describe('ReferenceGrant mutation adapter', () => {
  it('does not inject create defaults into an existing API view', () => {
    expect(normalize({
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'ReferenceGrant',
      metadata: { name: 'empty', namespace: 'default' },
      spec: {},
    }).spec).toEqual({})
  })

  it('preserves unknown operator fields and strips server-owned fields', () => {
    const resource = normalize({
      apiVersion: 'gateway.networking.k8s.io/v1beta1',
      kind: 'ReferenceGrant',
      metadata: { name: 'allow', namespace: 'default', resourceVersion: '4' },
      spec: {
        from: [{ group: 'gateway.networking.k8s.io', kind: 'HTTPRoute', namespace: 'app', future: true }],
        to: [{ group: '', kind: 'Service', name: '' }],
        futureSpec: { enabled: false },
      },
      status: { ignored: true },
    })

    expect(yaml.load(toYaml(resource, 'update'))).toEqual({
      apiVersion: 'gateway.networking.k8s.io/v1beta1',
      kind: 'ReferenceGrant',
      metadata: { name: 'allow', namespace: 'default', resourceVersion: '4' },
      spec: {
        from: [{ group: 'gateway.networking.k8s.io', kind: 'HTTPRoute', namespace: 'app', future: true }],
        to: [{ group: '', kind: 'Service', name: '' }],
        futureSpec: { enabled: false },
      },
    })
  })
})

it('rejects incorrect identity and preserves absent namespace for explicit validation', () => {
  expect(() => normalize({ kind: 'Secret', metadata: { name: 'wrong' }, spec: {} })).toThrow('Expected ReferenceGrant')
  expect(normalize({ kind: 'ReferenceGrant', apiVersion: 'gateway.networking.k8s.io/v1', metadata: { name: 'grant' }, spec: { from: [], to: [] } }).metadata).not.toHaveProperty('namespace')
})

it('validates bounded grants without restricting extension resource kinds or the core group', () => {
  const resource = createEmpty()
  resource.spec.from = [{ group: 'edgion.io', kind: 'EdgionBackend', namespace: 'app' }]
  resource.spec.to = [{ group: '', kind: 'Secret' }]
  expect(validateReferenceGrant(resource)).toEqual([])
  resource.spec.from = Array.from({ length: 17 }, () => ({ group: '', kind: 'Service', namespace: 'app' }))
  expect(validateReferenceGrant(resource).join(' ')).toContain('1 to 16')
  resource.spec.from = [{ group: '', kind: '*', namespace: '*' }]
  expect(validateReferenceGrant(resource)).toHaveLength(2)
  resource.spec.to = []
  expect(validateReferenceGrant(resource).join(' ')).toContain('spec.to')
})
