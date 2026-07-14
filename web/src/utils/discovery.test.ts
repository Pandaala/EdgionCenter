import { describe, expect, it } from 'vitest'
import { resolveServerDiscovery } from './discovery'

const kubernetesCapabilities = {
  userAdmin: false,
  roleAdmin: false,
  auditQuery: false,
  controllerHistory: true,
  nativeRbac: true,
  leaderElection: true,
  passwordLogin: false,
}

describe('resolveServerDiscovery', () => {
  it('preserves the explicit no-password Center contract', () => {
    expect(resolveServerDiscovery({
      success: true,
      data: { mode: 'center', capabilities: kubernetesCapabilities },
    })).toEqual({ mode: 'center', capabilities: kubernetesCapabilities })
  })

  it('fails closed instead of guessing Controller/password mode', () => {
    expect(() => resolveServerDiscovery({ success: false })).toThrow()
    expect(() => resolveServerDiscovery({ success: true, data: { mode: 'center' } })).toThrow()
    expect(() => resolveServerDiscovery({ success: true, data: { mode: 'unexpected' } })).toThrow()
  })
})
