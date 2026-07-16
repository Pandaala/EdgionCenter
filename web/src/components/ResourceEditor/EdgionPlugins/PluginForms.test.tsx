import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import EdgionPluginsForm from './EdgionPluginsForm'
import EdgionStreamPluginsForm from '../EdgionStreamPlugins/EdgionStreamPluginsForm'
import EdgionConfigDataForm from '../EdgionConfigData/EdgionConfigDataForm'

vi.mock('@/components/ResourceEditor/HTTPRoute/sections/MetadataSection', () => ({ default: () => null }))
vi.mock('../common/MetadataSection', () => ({ default: () => null }))

describe('structured plugin forms', () => {
  it('narrowly edits an HTTP plugin config without truncating entries or stages', () => {
    const onChange = vi.fn()
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins', metadata: { name: 'p', namespace: 'edge' },
      spec: {
        requestPlugins: [{ alias: 'limit.one', conditions: { run: [{ type: 'keyExist' }] }, type: 'RateLimit', config: { rate: 10, interval: '1s', future: false } }],
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
