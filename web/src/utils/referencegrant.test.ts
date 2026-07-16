import { describe, expect, it } from 'vitest'
import * as yaml from 'js-yaml'
import { normalize, toYaml } from './referencegrant'

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
