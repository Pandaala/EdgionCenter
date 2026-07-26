import { describe, expect, it } from 'vitest'
import { controllerResourcePath } from './center'

describe('Center Controller resource proxy paths', () => {
  it('encodes Controller ids and preserves resource scope', () => {
    expect(controllerResourcePath('east/controller-a', 'httproute', 'namespaced')).toBe(
      '/api/v1/proxy/east~controller-a/api/v1/namespaced/httproute',
    )
    expect(controllerResourcePath('east/controller-a', 'gatewayclass', 'cluster')).toBe(
      '/api/v1/proxy/east~controller-a/api/v1/cluster/gatewayclass',
    )
  })
})
