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

  it('narrowly edits GatewayConfig access-log whitelist arrays', () => {
    const onChange = vi.fn()
    const data: any = {
      apiVersion: 'edgion.io/v1alpha1',
      kind: 'EdgionGatewayConfig',
      metadata: { name: 'default' },
      spec: {
        maxBodySize: '32MiB',
        accessLogExtern: {
          unmaskedKeys: {
            header: ['x-request-id'],
            respHeader: ['content-type'],
            query: [],
            cookie: ['locale'],
            ctx: ['tenant'],
            futureSource: ['preserved'],
          },
          futurePolicy: { enabled: false },
        },
        futureSpec: { enabled: true },
      },
    }

    render(<EdgionGatewayConfigForm data={data} onChange={onChange} />)
    fireEvent.change(screen.getByTestId('edgiongatewayconfig-unmasked-header-0'), {
      target: { value: 'traceparent' },
    })

    const edited = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0]
    expect(edited.spec.accessLogExtern).toEqual({
      ...data.spec.accessLogExtern,
      unmaskedKeys: {
        ...data.spec.accessLogExtern.unmaskedKeys,
        header: ['traceparent'],
      },
    })
    expect(edited.spec.futureSpec).toEqual(data.spec.futureSpec)

    fireEvent.click(screen.getByTestId('edgiongatewayconfig-unmasked-query-add'))
    const added = onChange.mock.calls[onChange.mock.calls.length - 1]?.[0]
    expect(added.spec.accessLogExtern.unmaskedKeys.query).toEqual([''])
    expect(added.spec.accessLogExtern.unmaskedKeys.cookie).toEqual(['locale'])
    expect(added.spec.accessLogExtern.futurePolicy).toEqual({ enabled: false })
  })

  it('renders maxBodySize and omits the removed ReferenceGrant control', () => {
    const data: any = {
      apiVersion: 'edgion.io/v1alpha1',
      kind: 'EdgionGatewayConfig',
      metadata: { name: 'default' },
      spec: { maxBodySize: '32MiB' },
    }

    render(<EdgionGatewayConfigForm data={data} onChange={vi.fn()} />)
    expect(screen.getByDisplayValue('32MiB')).toBeInTheDocument()
    expect(screen.queryByText('Enable ReferenceGrant Validation')).not.toBeInTheDocument()
  })
})
