import { describe, expect, it } from 'vitest'
import {
  normalizeHTTPRoute,
  toHTTPRouteMutationDocument,
  validateHTTPRouteForMutation,
  withHTTPRouteMirrorTuningAnnotation,
} from './httproute'

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

  it('patches one mirror annotation without losing unrelated annotations', () => {
    const route: any = {
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'HTTPRoute',
      metadata: {
        name: 'route',
        namespace: 'edge',
        annotations: {
          'future.example.com/retained': 'yes',
          'edgion.io/mirror-log': 'true',
        },
      },
      spec: { parentRefs: [{ name: 'gateway' }] },
    }

    const updated = withHTTPRouteMirrorTuningAnnotation(route, 'connectTimeoutMs', '2500')
    expect(updated.metadata.annotations).toEqual({
      'future.example.com/retained': 'yes',
      'edgion.io/mirror-log': 'true',
      'edgion.io/mirror-connect-timeout-ms': '2500',
    })
    expect(withHTTPRouteMirrorTuningAnnotation(updated, 'connectTimeoutMs', undefined).metadata.annotations)
      .toEqual({
        'future.example.com/retained': 'yes',
        'edgion.io/mirror-log': 'true',
      })
    expect(route.metadata.annotations).not.toHaveProperty('edgion.io/mirror-connect-timeout-ms')
  })

  it('accepts RequestMirror percent and rejects invalid or mixed sampling fields', () => {
    const route: any = {
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'HTTPRoute',
      metadata: { name: 'route', namespace: 'edge' },
      spec: {
        parentRefs: [{ name: 'gateway' }],
        rules: [{
          filters: [{
            type: 'RequestMirror',
            requestMirror: { backendRef: { name: 'mirror' }, percent: 20 },
          }],
        }],
      },
    }

    expect(() => validateHTTPRouteForMutation(route)).not.toThrow()
    route.spec.rules[0].filters[0].requestMirror.percent = 101
    expect(() => validateHTTPRouteForMutation(route)).toThrow(/integer from 0 through 100/)
    route.spec.rules[0].filters[0].requestMirror.percent = 20
    route.spec.rules[0].filters[0].requestMirror.fraction = { numerator: 1, denominator: 2 }
    expect(() => validateHTTPRouteForMutation(route)).toThrow(/cannot combine percent and fraction/)
  })

  it('strips removed mirror and ExternalAuth fields while retaining unknown siblings', () => {
    const route: any = {
      apiVersion: 'gateway.networking.k8s.io/v1',
      kind: 'HTTPRoute',
      metadata: { name: 'route', namespace: 'edge' },
      spec: {
        parentRefs: [{ name: 'gateway' }],
        rules: [{
          filters: [
            {
              type: 'RequestMirror',
              requestMirror: {
                backendRef: { name: 'mirror' },
                percentage: 25,
                connectTimeoutMs: 500,
                futureMirrorField: { retained: true },
              },
            },
            {
              type: 'ExternalAuth',
              externalAuth: {
                target: { name: 'auth' },
                allowDegradation: true,
                allowDegradationTemplate: '${ctx:degrade}',
                futureAuthField: { retained: true },
              },
            },
          ],
          backendRefs: [{
            name: 'backend',
            filters: [{
              type: 'ExternalAuth',
              externalAuth: {
                target: { name: 'auth' },
                allowDegradation: false,
                futureBackendAuthField: true,
              },
            }],
          }],
        }],
      },
    }

    const mutation: any = toHTTPRouteMutationDocument(route, 'update')
    expect(mutation.spec.rules[0].filters[0].requestMirror).toEqual({
      backendRef: { name: 'mirror' },
      futureMirrorField: { retained: true },
    })
    expect(mutation.spec.rules[0].filters[1].externalAuth).toEqual({
      target: { name: 'auth' },
      futureAuthField: { retained: true },
    })
    expect(mutation.spec.rules[0].backendRefs[0].filters[0].externalAuth).toEqual({
      target: { name: 'auth' },
      futureBackendAuthField: true,
    })
  })
})
