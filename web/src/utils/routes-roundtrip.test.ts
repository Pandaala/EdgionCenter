import { describe, expect, it } from 'vitest'
import { normalizeGRPCRoute, grpcRouteToMutationYaml, validateGRPCRouteForMutation, yamlToGRPCRoute } from './grpcroute'
import { normalizeTCPRoute, tcpRouteToMutationYaml, yamlToTCPRoute } from './tcproute'
import { normalizeTLSRoute, tlsRouteToMutationYaml, yamlToTLSRoute } from './tlsroute'
import { normalizeUDPRoute, udpRouteToMutationYaml, yamlToUDPRoute } from './udproute'

describe('route mutation round trips', () => {
  it('preserves complete GRPCRoute rules and strips controller runtime state', () => {
    const route = normalizeGRPCRoute({
      apiVersion: 'gateway.networking.k8s.io/v1', kind: 'GRPCRoute',
      metadata: { name: 'grpc', namespace: 'edge', uid: 'server' },
      spec: {
        parentRefs: [{ group: 'gateway.networking.k8s.io', kind: 'Gateway', namespace: 'infra', name: 'gw', sectionName: 'grpc', port: 443 }],
        hostnames: ['grpc.example.com'],
        rules: [{
          name: 'primary',
          matches: [{ method: { type: 'RegularExpression', service: 'pkg\\..*', method: 'Get.*' }, headers: [{ type: 'Exact', name: 'x-tenant', value: 'blue' }] }],
          filters: [{ type: 'RequestHeaderModifier', requestHeaderModifier: { set: [{ name: 'x-set', value: 'yes' }] }, futureFilter: true }],
          backendRefs: [{ group: '', kind: 'Service', namespace: 'backends', name: 'grpc-api', port: 50051, weight: 80, filters: [{ type: 'ExtensionRef', extensionRef: { group: 'edgion.io', kind: 'EdgionPlugins', name: 'grpc-plugin' } }], futureBackend: true }],
          timeouts: { request: '30s', backendRequest: '10s' }, retry: { attempts: 3, backoff: '500ms', codes: [14] },
          sessionPersistence: { type: 'Header', sessionName: 'x-session', strict: true }, futureRule: true,
        }, { backendRefs: [{ name: 'fallback', port: 50052 }] }],
        resolvedRules: [{ runtime: true }], futureSpec: true,
      }, status: { parents: [] },
    })
    const mutation = yamlToGRPCRoute(grpcRouteToMutationYaml(route, 'update')) as any
    expect(mutation.spec.rules).toHaveLength(2)
    expect(mutation.spec.rules[0].backendRefs[0].filters[0].type).toBe('ExtensionRef')
    expect(mutation.spec.rules[0].futureRule).toBe(true)
    expect(mutation.spec.futureSpec).toBe(true)
    expect(mutation.spec.resolvedRules).toBeUndefined()
    expect(mutation.status).toBeUndefined()
    expect(mutation.metadata.uid).toBeUndefined()
  })

  it('enforces gRPC retry codes and service-level delegation constraints', () => {
    const route: any = { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'GRPCRoute', metadata: { name: 'g', namespace: 'n' }, spec: { rules: [{ matches: [{ method: { type: 'RegularExpression', service: 'pkg.Service', method: 'Get' } }], retry: { codes: [14] }, backendRefs: [{ group: 'gateway.networking.k8s.io', kind: 'GRPCRoute', name: 'child' }] }] } }
    expect(() => validateGRPCRouteForMutation(route)).toThrow(/Exact/)
    route.spec.rules[0].matches[0].method.type = 'Exact'
    expect(() => validateGRPCRouteForMutation(route)).toThrow(/must not set method.method/)
    delete route.spec.rules[0].matches[0].method.method
    route.spec.rules[0].retry.codes = [17]
    expect(() => validateGRPCRouteForMutation(route)).toThrow(/0 through 16/)
  })

  it.each([
    ['TCPRoute', normalizeTCPRoute, tcpRouteToMutationYaml, yamlToTCPRoute, undefined],
    ['UDPRoute', normalizeUDPRoute, udpRouteToMutationYaml, yamlToUDPRoute, undefined],
    ['TLSRoute', normalizeTLSRoute, tlsRouteToMutationYaml, yamlToTLSRoute, ['one.example.com', '*.example.com']],
  ] as const)('preserves every %s rule, backend ref field, and unknown operator field', (_kind, normalize, toYaml, fromYaml, hostnames) => {
    const route = normalize({
      apiVersion: _kind === 'TLSRoute' ? 'gateway.networking.k8s.io/v1' : 'gateway.networking.k8s.io/v1alpha2',
      kind: _kind,
      metadata: { name: 'stream', namespace: 'edge', annotations: { 'edgion.io/proxy-protocol': '2' } },
      spec: {
        parentRefs: [{ group: 'gateway.networking.k8s.io', kind: 'Gateway', namespace: 'infra', name: 'gw', sectionName: 'stream', port: 9443 }],
        ...(hostnames ? { hostnames } : {}),
        rules: [
          { backendRefs: [{ group: '', kind: 'Service', namespace: 'backends', name: 'one', port: 9443, weight: 80, futureBackend: true }], futureRule: true },
          { backendRefs: [{ name: 'two', port: 9444, weight: 20 }] },
        ],
        futureSpec: true,
      },
    } as any)
    const mutation = fromYaml(toYaml(route as any, 'update')) as any
    expect(mutation.spec.rules).toHaveLength(2)
    expect(mutation.spec.rules[0].backendRefs[0].futureBackend).toBe(true)
    expect(mutation.spec.rules[0].futureRule).toBe(true)
    expect(mutation.spec.futureSpec).toBe(true)
    if (hostnames) expect(mutation.spec.hostnames).toEqual(hostnames)
  })
})
