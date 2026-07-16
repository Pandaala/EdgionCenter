import * as yaml from 'js-yaml'
import { describe, expect, it } from 'vitest'
import { edgionPluginsToMutationYAML, edgionPluginsToYAML, normalizeEdgionPlugins, yamlToEdgionPlugins } from './edgionplugins'

describe('EdgionPlugins lossless adapter', () => {
  it('preserves all four stages, aliases, conditions, unknown fields, and cardinality', () => {
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins',
      metadata: { name: 'plugins', namespace: 'edge', resourceVersion: '7' },
      spec: {
        requestPlugins: [{ enable: false, alias: 'auth.1', conditions: { run: [{ type: 'keyExist' }] }, type: 'Wasm', config: { source: { type: 'inline', module: 'AA==' }, resolvedPullSecret: '[redacted]', future: false } }],
        upstreamResponseFilterPlugins: [{ type: 'Dsl', config: { name: 'response', source: 'return;' } }],
        upstreamResponseBodyFilterPlugins: [{ type: 'BandwidthLimit', config: { rate: '1mb' } }],
        upstreamResponsePlugins: [{ type: 'ExtProc', config: { grpcService: { target: 'proc:9000' }, processingMode: {} } }],
        futureSpec: { preserve: [] },
      },
      status: { conditions: [] },
    }
    expect(yamlToEdgionPlugins(edgionPluginsToYAML(normalizeEdgionPlugins(resource)))).toEqual(resource)
    const mutation: any = yaml.load(edgionPluginsToMutationYAML(resource, 'update'))
    expect(mutation.metadata.resourceVersion).toBe('7')
    expect(mutation.status).toBeUndefined()
    expect(mutation.spec.futureSpec).toEqual({ preserve: [] })
    expect(mutation.spec.requestPlugins[0].alias).toBe('auth.1')
    expect(mutation.spec.requestPlugins[0].config.resolvedPullSecret).toBeUndefined()
    expect(mutation.spec.requestPlugins[0].config.future).toBe(false)
  })
})
