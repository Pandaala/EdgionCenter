import { describe, expect, it } from 'vitest'
import { grpcRouteToMutationYaml, normalizeGRPCRoute, yamlToGRPCRoute } from './grpcroute'

function route(filters: unknown, backend = false) {
  return normalizeGRPCRoute({
    apiVersion: 'gateway.networking.k8s.io/v1', kind: 'GRPCRoute',
    metadata: { name: 'grpc', namespace: 'edge', resourceVersion: '13' },
    spec: { rules: [{
      ...(backend ? {} : { filters }),
      backendRefs: [{ name: 'api', port: 50051, weight: 0, ...(backend ? { filters } : {}) }],
      futureRule: { retained: true },
    }] },
  })
}

describe.each([false, true])('GRPCRoute filter preflight (backend=%s)', (backend) => {
  const path = backend ? 'rules[0].backendRefs[0].filters' : 'rules[0].filters'

  it.each(['RequestMirror', 'RequestRedirect', 'URLRewrite', 'CORS', 'ExternalAuth', 'FutureFilter'])('rejects unsupported %s with its exact location and retains the draft', (type) => {
    const draft = route([{ type, futureFilter: true }], backend)
    const before = structuredClone(draft)
    expect(() => grpcRouteToMutationYaml(draft, 'update')).toThrow(`${path}[0].type must be a supported GRPCRoute filter`)
    expect(draft).toEqual(before)
  })

  it('retains all supported filter payloads and unknown operator fields', () => {
    const filters = [
      { type: 'RequestHeaderModifier', requestHeaderModifier: { set: [{ name: 'x-added', value: 'yes' }] }, futureFilter: false },
      { type: 'ResponseHeaderModifier', responseHeaderModifier: { remove: ['x-internal'] } },
      { type: 'ExtensionRef', extensionRef: { group: 'edgion.io', kind: 'EdgionPlugins', name: 'plugins' } },
    ]
    const draft = route(filters, backend)
    expect(yamlToGRPCRoute(grpcRouteToMutationYaml(draft, 'update'))).toEqual(draft)
  })

  it('rejects malformed filter containers and entries without coercion', () => {
    expect(() => grpcRouteToMutationYaml(route({}, backend), 'create')).toThrow(`${path} must be an array`)
    for (const entry of [null, [], {}, { type: 1 }]) {
      expect(() => grpcRouteToMutationYaml(route([entry], backend), 'create')).toThrow(`${path}[0].type`)
    }
  })
})
