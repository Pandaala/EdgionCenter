import { describe, expect, it } from 'vitest'
import { normalizeHTTPRoute, toHTTPRouteMutationDocument, validateHTTPRouteForMutation } from './httproute'

describe('HTTPRoute lossless adapter', () => {
  it('preserves rule extensions and backend filters while stripping runtime state', () => {
    const fixture = {
      apiVersion: 'gateway.networking.k8s.io/v1' as const,
      kind: 'HTTPRoute' as const,
      metadata: { name: 'route', namespace: 'edge', annotations: { future: '' }, uid: 'server' },
      spec: {
        parentRefs: [{ name: 'gateway', futureParent: true }],
        rules: [
          {
            name: 'primary',
            matches: [],
            retry: { attempts: 3, backoff: '10ms', codes: [502, 503], futureRetry: true },
            sessionPersistence: { type: 'Cookie', strict: false, cookieConfig: { lifetimeType: 'Session' } },
            backendRefs: [{ name: 'api', port: 80, filters: [{ type: 'RequestHeaderModifier', requestHeaderModifier: { add: [{ name: 'x-route', value: 'one' }] } }], refDenied: { reason: 'runtime' }, futureBackend: [] }],
            parsedRetry: { runtime: true },
            futureRule: { retained: true },
          },
          { name: 'fallback', backendRefs: [{ name: 'fallback', port: 8080 }] },
        ],
        futureSpec: false,
        resolvedRules: [{ runtime: true }],
      },
      status: { parents: [] },
    }

    const view = normalizeHTTPRoute(fixture)
    const mutation = toHTTPRouteMutationDocument(view, 'update')

    expect(view).toEqual(fixture)
    expect(mutation).toHaveProperty('spec.rules.0.name', 'primary')
    expect(mutation).toHaveProperty('spec.rules.0.retry.futureRetry', true)
    expect(mutation).toHaveProperty('spec.rules.0.sessionPersistence.strict', false)
    expect(mutation).toHaveProperty('spec.rules.0.backendRefs.0.filters.0.type', 'RequestHeaderModifier')
    expect(mutation).toHaveProperty('spec.rules.0.backendRefs.0.futureBackend', [])
    expect(mutation).toHaveProperty('spec.rules.1.name', 'fallback')
    expect(mutation).not.toHaveProperty('spec.rules.0.parsedRetry')
    expect(mutation).not.toHaveProperty('spec.rules.0.backendRefs.0.refDenied')
    expect(mutation).not.toHaveProperty('spec.resolvedRules')
    expect(mutation).not.toHaveProperty('status')
  })

  it('rejects redirect/rewrite conflicts at rule, backend, and cross levels', () => {
    const base: any = { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'HTTPRoute', metadata: { name: 'r', namespace: 'n' }, spec: { rules: [] } }
    for (const rule of [
      { filters: [{ type: 'RequestRedirect' }, { type: 'URLRewrite' }] },
      { backendRefs: [{ name: 'b', filters: [{ type: 'RequestRedirect' }, { type: 'URLRewrite' }] }] },
      { filters: [{ type: 'RequestRedirect' }], backendRefs: [{ name: 'b', filters: [{ type: 'URLRewrite' }] }] },
    ]) {
      expect(() => validateHTTPRouteForMutation({ ...base, spec: { rules: [rule] } })).toThrow(/cannot combine/)
    }
  })

  it('enforces HTTP retry-code and delegation constraints', () => {
    const route: any = { apiVersion: 'gateway.networking.k8s.io/v1', kind: 'HTTPRoute', metadata: { name: 'r', namespace: 'n' }, spec: { rules: [{ matches: [{ path: { type: 'Exact', value: '/x' } }], retry: { codes: [99] }, backendRefs: [{ group: 'gateway.networking.k8s.io', kind: 'HTTPRoute', name: 'child', port: 80 }] }] } }
    expect(() => validateHTTPRouteForMutation(route)).toThrow(/port/)
    delete route.spec.rules[0].backendRefs[0].port
    expect(() => validateHTTPRouteForMutation(route)).toThrow(/PathPrefix/)
    route.spec.rules[0].matches[0].path.type = 'PathPrefix'
    expect(() => validateHTTPRouteForMutation(route)).toThrow(/100 through 599/)
  })
})
