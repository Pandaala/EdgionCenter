import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import EdgionPluginsForm from './EdgionPluginsForm'
import EdgionStreamPluginsForm from '../EdgionStreamPlugins/EdgionStreamPluginsForm'
import EdgionConfigDataForm from '../EdgionConfigData/EdgionConfigDataForm'

vi.mock('@/components/ResourceEditor/HTTPRoute/sections/MetadataSection', () => ({ default: () => null }))
vi.mock('../common/MetadataSection', () => ({ default: () => null }))

describe('structured plugin forms', () => {
  it('edits connection GeoIP rules without changing TLSRoute stage entries', () => {
    const onChange = vi.fn()
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionStreamPlugins', metadata: { name: 'geo', namespace: 'edge' },
      spec: {
        plugins: [{ type: 'GeoIpLocation', config: { defaultAction: 'allow', failOpen: false, future: [] } }],
        tlsRoutePlugins: [{ type: 'IpRestriction', config: { ipSource: 'DirectPeerIp', future: true } }],
      },
    }
    render(<EdgionStreamPluginsForm data={resource} onChange={onChange} />)
    expect(screen.getByText(/GeoIP rules must use DirectPeerIp/)).toBeInTheDocument()
    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'failOpen' }))
    fireEvent.click(screen.getByText('true', { selector: '.ant-select-item-option-content' }))
    expect(onChange).toHaveBeenCalledWith({
      ...resource,
      spec: { ...resource.spec, plugins: [{ ...resource.spec.plugins[0], config: { ...resource.spec.plugins[0].config, failOpen: true } }] },
    })
  })

  it('narrowly edits an HTTP plugin config without truncating entries or stages', () => {
    const onChange = vi.fn()
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins', metadata: { name: 'p', namespace: 'edge' },
      spec: {
        requestPlugins: [{ alias: 'limit.one', conditions: { run: { allOf: [{ type: 'keyExist', key: { type: 'header', name: 'x-tenant' } }] } }, type: 'RateLimitLocal', config: { rate: 10, interval: '1s', future: false } }],
        upstreamResponsePlugins: [{ type: 'ExtProc', config: { grpcService: { target: 'proc:9000' } } }],
        futureSpec: true,
      },
    }
    render(<EdgionPluginsForm value={resource} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue('1s'), { target: { value: '2s' } })
    expect(onChange).toHaveBeenCalledWith({
      ...resource,
      spec: {
        ...resource.spec,
        requestPlugins: [{
          ...resource.spec.requestPlugins[0],
          config: { rate: 10, interval: '2s', future: false },
        }],
      },
    })
  })

  it('narrowly edits access-log declarations while preserving entry-level dye, body, and unknown siblings', () => {
    const onChange = vi.fn()
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins', metadata: { name: 'p', namespace: 'edge' },
      spec: {
        requestPlugins: [{
          alias: 'auth',
          dye: { request: [{ name: 'x-auth', on: ['success'] }], futureDye: true },
          body: { maxBodySize: '1m', onReadFailure: 'failClose', futureBody: false },
          type: 'HmacAuth',
          config: { validateRequestBody: true },
          futureEntry: [],
        }],
        accessLogExtern: [{ key: 'tenant', from: 'routeLabel', name: 'tenant', futureField: 1 }],
        futureSpec: true,
      },
    }
    render(<EdgionPluginsForm value={resource} onChange={onChange} />)
    fireEvent.change(screen.getAllByDisplayValue('tenant')[0], { target: { value: 'tenant-id' } })
    expect(onChange).toHaveBeenCalledWith({
      ...resource,
      spec: {
        ...resource.spec,
        accessLogExtern: [{
          ...resource.spec.accessLogExtern[0],
          key: 'tenant-id',
        }],
      },
    })
  })

  it('edits Stage 1 while preserving TLSRoute and flattened entry fields', () => {
    const onChange = vi.fn()
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionStreamPlugins', metadata: { name: 's', namespace: 'edge' },
      spec: {
        plugins: [{ enable: false, type: 'ConnectionRateLimit', futureEntry: true, config: { redisRef: 'edge/redis', future: 0 } }],
        tlsRoutePlugins: [{ type: 'IpRestriction', config: { status: 403, futureTls: [] } }],
      },
    }
    render(<EdgionStreamPluginsForm data={resource} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue('edge/redis'), { target: { value: 'edge/redis-new' } })
    expect(onChange).toHaveBeenCalledWith({
      ...resource,
      spec: {
        ...resource.spec,
        plugins: [{ ...resource.spec.plugins[0], config: { redisRef: 'edge/redis-new', future: 0 } }],
      },
    })
  })

  it.each([
    ['RequestAccessUrlAllowList', { items: [{ name: 'health', paths: [{ type: 'Exact', value: '/health' }] }] }, '/health', '/ready'],
    ['ProxyProtocolTrust', { mode: 'trustedSources', trustedCidrs: ['192.0.2.0/24'] }, '192.0.2.0/24', '198.51.100.0/24'],
    ['WafRuleBundle', { version: 'one', profile: 'local', provenance: 'operator', roots: ['main.conf'], rules: [{ name: 'main.conf', content: 'SecRuleEngine On' }] }, 'operator', 'repository'],
    ['WafPolicy', { defaultProfile: 'base', profiles: { base: { bundleRefs: [{ name: 'rules', optional: false }] } } }, 'rules', 'updated-rules'],
  ] as const)('edits %s while retaining the complete envelope', (type, config, before, after) => {
    const onChange = vi.fn()
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData',
      metadata: { name: 'typed', namespace: 'edge', resourceVersion: '42' },
      spec: { enable: true, visibility: 'Namespace', data: { type, config, futureEnvelope: false } },
    }
    render(<EdgionConfigDataForm data={resource} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue(before), { target: { value: after } })
    const next = onChange.mock.lastCall?.[0]
    expect(next.metadata).toEqual(resource.metadata)
    expect(next.spec.data.type).toBe(type)
    expect(next.spec.data.futureEnvelope).toBe(false)
    expect(JSON.stringify(next.spec.data.config)).toBe(JSON.stringify(config).replace(before, after))
  })

  it('edits typed ConfigData fields without a YAML/JSON textarea', () => {
    const onChange = vi.fn()
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData', metadata: { name: 'selector', namespace: 'edge' },
      spec: { enable: true, visibility: 'Namespace', data: { type: 'Selector', futureEnvelope: true, config: { active: 'safe', description: 'base', future: false } } },
    }
    render(<EdgionConfigDataForm data={resource} onChange={onChange} />)
    expect(screen.queryByRole('textbox', { name: /YAML/i })).not.toBeInTheDocument()
    fireEvent.change(screen.getByDisplayValue('safe'), { target: { value: 'emergency' } })
    expect(onChange).toHaveBeenCalledWith({
      ...resource,
      spec: {
        ...resource.spec,
        data: { ...resource.spec.data, config: { active: 'emergency', description: 'base', future: false } },
      },
    })
  })
})
