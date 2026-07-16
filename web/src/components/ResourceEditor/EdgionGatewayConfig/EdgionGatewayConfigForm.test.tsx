import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import EdgionGatewayConfigForm from './EdgionGatewayConfigForm'

describe('EdgionGatewayConfigForm', () => {
  it('narrowly edits security protection while preserving every sibling module and unknown field', () => {
    const onChange = vi.fn()
    const data: any = {
      apiVersion: 'edgion.io/v1alpha1', kind: 'EdgionGatewayConfig', metadata: { name: 'default' },
      spec: {
        securityProtect: { xForwardedForLimit: 200, requireSniHostMatch: true, allowLoopbackUpstream: false, futureSecurity: [] },
        realIp: { trustedIps: [{ name: 'private', cidrs: ['10.0.0.0/8'] }] },
        globalPluginsRef: [{ name: 'one' }, { name: 'two' }],
        outboundTls: { verify: true, validation: { hostname: 'api.example.com' } },
        dnsResolver: { servers: ['1.1.1.1'], cacheTtl: '5s' },
        futureSpec: { enabled: false },
      },
    }

    render(<EdgionGatewayConfigForm data={data} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue('200'), { target: { value: '201' } })

    const next = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0]
    expect(next.spec.securityProtect).toEqual({ ...data.spec.securityProtect, xForwardedForLimit: 201 })
    expect(next.spec.realIp).toEqual(data.spec.realIp)
    expect(next.spec.globalPluginsRef).toHaveLength(2)
    expect(next.spec.outboundTls).toEqual(data.spec.outboundTls)
    expect(next.spec.dnsResolver).toEqual(data.spec.dnsResolver)
    expect(next.spec.futureSpec).toEqual({ enabled: false })
  })
})
