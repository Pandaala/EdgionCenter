import { describe, expect, it } from 'vitest'
import { validateRouteHostnames } from './route-hostnames'
import { toHTTPRouteMutationDocument } from './httproute'
import { grpcRouteToMutationYaml } from './grpcroute'
import { tlsRouteToMutationYaml } from './tlsroute'

describe('current Controller route hostname contract', () => {
  it.each(['HTTPRoute', 'GRPCRoute', 'TLSRoute'] as const)('%s preserves supported patterns and enforces its list boundary', (kind) => {
    const valid = ['localhost', '*.example.com', '*.127.0.0.1', '01.2.3.4', '256.1.1.1', 'a'.repeat(253)]
    expect(() => validateRouteHostnames(valid, kind)).not.toThrow()
    expect(() => validateRouteHostnames(Array(kind === 'TLSRoute' ? 1024 : 16).fill('a'), kind)).not.toThrow()
    expect(() => validateRouteHostnames(Array(kind === 'TLSRoute' ? 1025 : 17).fill('a'), kind)).toThrow(/at most/)
    for (const missing of [undefined, null, []]) {
      if (kind === 'TLSRoute') expect(() => validateRouteHostnames(missing, kind)).toThrow(/at least one/)
      else expect(() => validateRouteHostnames(missing, kind)).not.toThrow()
    }
  })

  it.each(['', '*', 'UPPER.example', 'example.com.', '-bad.example', 'bad-.example', 'a..b', 'a.*.b', 'a_b', 'example.com\n', 'example.com\r', '127.0.0.1', '0.0.0.0', '255.255.255.255', '::1', '[::1]', 'a'.repeat(254), 123])('rejects invalid authored hostname %s', (name) => {
    expect(() => validateRouteHostnames([name], 'TLSRoute')).toThrow(/spec.hostnames\[0\]/)
  })

  it.each(['example.com', 123, {}])('rejects a non-array hostname list %s', (hostnames) => {
    expect(() => validateRouteHostnames(hostnames, 'HTTPRoute')).toThrow(/must be an array/)
  })

  it.each([
    ['HTTPRoute', toHTTPRouteMutationDocument],
    ['GRPCRoute', grpcRouteToMutationYaml],
    ['TLSRoute', tlsRouteToMutationYaml],
  ] as const)('enforces %s hostname validation at both mutation boundaries without changing input', (kind, serialize) => {
    const resource: any = { apiVersion: 'gateway.networking.k8s.io/v1', kind, metadata: { name: 'route', namespace: 'default' }, spec: { hostnames: ['127.0.0.1'], rules: [], futureField: { keep: true } } }
    const before = structuredClone(resource)
    for (const mode of ['create', 'update'] as const) expect(() => serialize(resource, mode)).toThrow(/IP address/)
    expect(resource).toEqual(before)
    resource.spec.hostnames = ['*.example.com']
    for (const mode of ['create', 'update'] as const) expect(() => serialize(resource, mode)).not.toThrow()
  })
})
