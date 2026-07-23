import * as yaml from 'js-yaml'
import { describe, expect, it } from 'vitest'
import {
  edgionPluginsToMutationYAML,
  edgionPluginsToYAML,
  normalizeEdgionPlugins,
  pluginAcceptsBodyRequirement,
  validateAccessLogExtern,
  validatePluginBodyRequirements,
  yamlToEdgionPlugins,
} from './edgionplugins'

describe('EdgionPlugins lossless adapter', () => {
  it('preserves all four stages, aliases, conditions, unknown fields, and cardinality', () => {
    const resource: any = {
      apiVersion: 'edgion.io/v1', kind: 'EdgionPlugins',
      metadata: { name: 'plugins', namespace: 'edge', resourceVersion: '7' },
      spec: {
        requestPlugins: [{
          enable: false,
          alias: 'auth.1',
          conditions: { run: [{ type: 'keyExist' }] },
          body: { maxBodySize: '1m', onReadFailure: 'failClose', futureBody: true },
          dye: { request: [{ name: 'x-region', on: ['success'] }], futureDye: false },
          policyAction: { action: 'deny' },
          type: 'Wasm',
          config: { source: { type: 'inline', module: 'AA==' }, resolvedPullSecret: '[redacted]', future: false },
        }],
        upstreamResponseFilterPlugins: [{ dye: { response: [{ name: 'x-result', on: ['success'] }] }, type: 'Dsl', config: { name: 'response', source: 'return;' } }],
        upstreamResponseBodyFilterPlugins: [{ type: 'BandwidthLimit', config: { rate: '1mb' } }],
        upstreamResponsePlugins: [{ type: 'ExtProc', config: { grpcService: { target: 'proc:9000' }, processingMode: {} } }],
        accessLogExtern: [{ key: 'tenant', from: 'routeLabel', name: 'tenant' }],
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
    expect(mutation.spec.requestPlugins[0].body.futureBody).toBe(true)
    expect(mutation.spec.requestPlugins[0].dye.futureDye).toBe(false)
    expect(mutation.spec.requestPlugins[0].policyAction).toBeUndefined()
    expect(mutation.spec.requestPlugins[0].config.resolvedPullSecret).toBeUndefined()
    expect(mutation.spec.requestPlugins[0].config.future).toBe(false)
    expect(mutation.spec.accessLogExtern).toEqual([{ key: 'tenant', from: 'routeLabel', name: 'tenant' }])
  })
})

describe('EdgionPlugins accessLogExtern validation', () => {
  it('accepts all current source variants', () => {
    const sources = ['routeLabel', 'routeAnnotation', 'header', 'query', 'cookie', 'respHeader', 'ctx'] as const
    expect(validateAccessLogExtern(sources.map((from, index) => ({
      key: `field-${index}`,
      from,
      name: from === 'routeAnnotation' ? 'owner' : `source-${index}`,
    })))).toEqual([])
  })

  it('rejects count, byte length, duplicate, credential, ctx, and annotation violations', () => {
    const fields: any[] = Array.from({ length: 17 }, (_, index) => ({
      key: `key-${index}`,
      from: 'query',
      name: `name-${index}`,
    }))
    fields[0] = { key: '', from: 'header', name: 'Authorization' }
    fields[1] = { key: 'duplicate', from: 'query', name: 'first' }
    fields[2] = { key: 'duplicate', from: 'query', name: 'second' }
    fields[3] = { key: 'ctx', from: 'ctx', name: 'jwt_claims' }
    fields[4] = { key: 'annotation', from: 'routeAnnotation', name: 'edgion.io/private' }
    fields[5] = { key: 'é'.repeat(32), from: 'query', name: 'ok' }
    const errors = validateAccessLogExtern(fields)
    expect(errors).toEqual(expect.arrayContaining([
      expect.stringContaining('at most 16 fields'),
      expect.stringContaining('key must not be empty'),
      expect.stringContaining('Authorization'),
      expect.stringContaining("duplicate key 'duplicate'"),
      expect.stringContaining('jwt_claims'),
      expect.stringContaining('edgion.io/private'),
      expect.stringContaining('key must be at most 63 bytes'),
    ]))
  })

  it('rejects malformed declarations and unknown sources with stable errors', () => {
    expect(validateAccessLogExtern([
      { key: 'secret', from: 'secret', name: 'token' },
      { key: 1, from: 'query' },
      null,
    ] as any)).toEqual(expect.arrayContaining([
      expect.stringContaining('from must be one of'),
      expect.stringContaining('key must be a string'),
      expect.stringContaining('name must be a string'),
      expect.stringContaining('field must be an object'),
    ]))
  })

  it('rejects an invalid declaration before mutation', () => {
    const resource = normalizeEdgionPlugins({
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionPlugins',
      metadata: { name: 'plugins', namespace: 'edge' },
      spec: {
        accessLogExtern: [{ key: 'credential', from: 'header', name: 'Authorization' }],
      },
    })

    expect(() => edgionPluginsToMutationYAML(resource, 'update')).toThrow(
      /Authorization.*blocked/,
    )
  })
})

describe('EdgionPlugins body requirement capability', () => {
  it('mirrors current request-stage Wasm, HmacAuth, and DSL capability gates', () => {
    expect(pluginAcceptsBodyRequirement('requestPlugins', 'Wasm', {})).toBe(true)
    expect(pluginAcceptsBodyRequirement('requestPlugins', 'HmacAuth', { validateRequestBody: true })).toBe(true)
    expect(pluginAcceptsBodyRequirement('requestPlugins', 'HmacAuth', { validateRequestBody: false })).toBe(false)
    expect(pluginAcceptsBodyRequirement('requestPlugins', 'Dsl', { source: 'let body = req.body' })).toBe(true)
    expect(pluginAcceptsBodyRequirement('requestPlugins', 'Dsl', { source: 'let host = req.host' })).toBe(false)
    expect(pluginAcceptsBodyRequirement('requestPlugins', 'Dsl', { source: '// req.body' })).toBe(true)
    expect(pluginAcceptsBodyRequirement('requestPlugins', 'Dsl', { bytecode: 'opaque' })).toBe(true)
    expect(pluginAcceptsBodyRequirement('upstreamResponsePlugins', 'ExtProc', {})).toBe(false)
    expect(pluginAcceptsBodyRequirement('requestPlugins', 'TraceContext', {})).toBe(false)
  })

  it('rejects stale body blocks on plugins that do not consume a body', () => {
    const spec: any = {
      requestPlugins: [
        { type: 'TraceContext', config: {}, body: { maxBodySize: '1MiB' } },
        { type: 'HmacAuth', config: { validateRequestBody: true }, body: { maxBodySize: '1MiB' } },
      ],
    }
    expect(validatePluginBodyRequirements(spec)).toEqual([
      'requestPlugins[0]: plugin TraceContext does not accept an operator body requirement',
    ])
    spec.requestPlugins[0].enable = false
    expect(validatePluginBodyRequirements(spec)).toEqual([])
    spec.requestPlugins[0].enable = true

    const resource = normalizeEdgionPlugins({
      apiVersion: 'edgion.io/v1',
      kind: 'EdgionPlugins',
      metadata: { name: 'plugins', namespace: 'edge' },
      spec,
    })
    expect(() => edgionPluginsToMutationYAML(resource, 'update')).toThrow(/TraceContext.*does not accept/)
  })
})
