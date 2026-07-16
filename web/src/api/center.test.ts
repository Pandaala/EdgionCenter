import { describe, expect, it } from 'vitest'
import { controllerDiagnosticsPath, controllerResourcePath, parseControllerConfConflicts } from './center'

describe('Center Controller resource proxy paths', () => {
  it('encodes Controller ids and preserves resource scope', () => {
    expect(controllerResourcePath('east/controller-a', 'httproute', 'namespaced')).toBe(
      '/api/v1/proxy/east~controller-a/api/v1/namespaced/httproute',
    )
    expect(controllerResourcePath('east/controller-a', 'gatewayclass', 'cluster')).toBe(
      '/api/v1/proxy/east~controller-a/api/v1/cluster/gatewayclass',
    )
  })
  it('builds the selected-Controller conflict diagnostics path', () => {
    expect(controllerDiagnosticsPath('east/controller-a')).toBe('/api/v1/proxy/east~controller-a/api/v1/diagnostics/conf-conflicts')
  })
  it('validates diagnostics and rejects malformed older responses', () => {
    expect(parseControllerConfConflicts({ conflicts: [{ kind: 'HTTPRoute', key: 'n/r', winner: '/a', losers: ['/b'] }] }).conflicts).toHaveLength(1)
    expect(() => parseControllerConfConflicts({ conflicts: [{ kind: 'HTTPRoute' }] })).toThrow()
    expect(() => parseControllerConfConflicts({})).toThrow()
  })
})
