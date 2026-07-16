import { describe, expect, it } from 'vitest'
import { normalize, toMutationDocument, validateGatewayClass } from './gatewayclass'

describe('GatewayClass lossless adapter', () => {
  it('preserves parametersRef, metadata, unknown fields, and strips status/server metadata', () => {
    const fixture = {
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'GatewayClass',
      metadata: { name: 'edgion', labels: { managed: 'yes' }, annotations: { note: '' }, resourceVersion: '3' },
      spec: {
        controllerName: 'edgion.io/gateway-controller',
        description: '',
        parametersRef: { group: 'edgion.io', kind: 'EdgionGatewayConfig', name: 'default', namespace: 'edge', futureRef: true },
        futureSpec: [],
      },
      status: { conditions: [] },
    }

    const view = normalize(fixture)
    const mutation = toMutationDocument(view, 'update')

    expect(view).toEqual(fixture)
    expect(mutation).toHaveProperty('metadata.labels.managed', 'yes')
    expect(mutation).toHaveProperty('metadata.annotations.note', '')
    expect(mutation).toHaveProperty('spec.parametersRef.futureRef', true)
    expect(mutation).toHaveProperty('spec.futureSpec', [])
    expect(mutation).toHaveProperty('metadata.resourceVersion', '3')
    expect(mutation).not.toHaveProperty('status')
  })

  it('accepts only the Edgion cluster-scoped parameters reference', () => {
    const resource: any = { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'GatewayClass', metadata: { name: 'edgion' }, spec: { controllerName: 'edgion.io/gateway-controller', parametersRef: { group: 'edgion.io', kind: 'EdgionGatewayConfig', name: 'default' } } }
    expect(validateGatewayClass(resource)).toEqual([])
    resource.spec.parametersRef = { group: 'other.io', kind: 'Config', name: '', namespace: 'default' }
    const errors = validateGatewayClass(resource).join('\n')
    expect(errors).toContain('group must be edgion.io')
    expect(errors).toContain('kind must be EdgionGatewayConfig')
    expect(errors).toContain('namespace is not allowed')
  })
})
