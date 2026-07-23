import { describe, expect, it } from 'vitest'
import { PLUGIN_TYPES, STAGE_PLUGIN_TYPES } from '@/types/edgion-plugins'
import {
  HTTP_PLUGIN_CATALOG,
  pluginTypesForStage,
} from './pluginCatalog'

describe('current Rust HTTP plugin catalog', () => {
  it('contains exactly the 39 EdgionPlugin variants and no nested ExtensionRef', () => {
    expect(HTTP_PLUGIN_CATALOG).toHaveLength(39)
    expect(new Set(HTTP_PLUGIN_CATALOG.map((entry) => entry.type)).size).toBe(39)
    expect(PLUGIN_TYPES).toHaveLength(39)
    expect([...PLUGIN_TYPES]).toEqual(HTTP_PLUGIN_CATALOG.map((entry) => entry.type))
    expect(PLUGIN_TYPES).not.toContain('ExtensionRef')
  })

  it('matches the four Rust applicable_stages sets', () => {
    expect(STAGE_PLUGIN_TYPES.requestPlugins).toEqual(pluginTypesForStage('requestPlugins'))
    expect(STAGE_PLUGIN_TYPES.upstreamResponseFilterPlugins).toEqual(pluginTypesForStage('upstreamResponseFilterPlugins'))
    expect(STAGE_PLUGIN_TYPES.upstreamResponseBodyFilterPlugins).toEqual(pluginTypesForStage('upstreamResponseBodyFilterPlugins'))
    expect(STAGE_PLUGIN_TYPES.upstreamResponsePlugins).toEqual(pluginTypesForStage('upstreamResponsePlugins'))
    expect(pluginTypesForStage('upstreamResponsePlugins')).toEqual(['ExtProc'])
    expect(pluginTypesForStage('upstreamResponseBodyFilterPlugins')).toEqual(['BandwidthLimit', 'Wasm'])
    expect(pluginTypesForStage('upstreamResponseFilterPlugins')).toEqual([
      'ResponseHeaderModifier', 'DebugAccessLogToHeader', 'ResponseRewrite', 'Dsl', 'Wasm',
    ])
    expect(pluginTypesForStage('requestPlugins')).toHaveLength(35)
  })

  it('publishes a structured field catalog for every non-empty Rust config', () => {
    const fieldless = HTTP_PLUGIN_CATALOG.filter((entry) => entry.fields.length === 0).map((entry) => entry.type)
    expect(fieldless).toEqual(['DebugAccessLogToHeader'])
    expect(HTTP_PLUGIN_CATALOG.find((entry) => entry.type === 'OpenidConnect')?.fields.length).toBeGreaterThan(50)
    expect(HTTP_PLUGIN_CATALOG.find((entry) => entry.type === 'Wasm')?.fields.map((field) => field.name)).toEqual([
      'source', 'sha256', 'pluginConfig', 'vmConfig', 'failOpen', 'timeoutMs',
      'instancePoolSize', 'calloutTimeoutMs', 'calloutAllowlist',
    ])
    expect(HTTP_PLUGIN_CATALOG.find((entry) => entry.type === 'RequestMirror')?.fields.map((field) => field.name)).toEqual([
      'backendRef', 'fraction', 'percent', 'connectTimeoutMs', 'writeTimeoutMs',
      'maxBufferedChunks', 'maxConcurrent', 'channelFullTimeoutMs', 'mirrorLog',
    ])
  })

  it('does not expose the removed config-level dyeHeaders fields', () => {
    for (const type of [
      'DirectEndpoint',
      'DynamicInternalUpstream',
      'DynamicExternalUpstream',
      'RegionRoute',
      'Canary',
    ]) {
      expect(HTTP_PLUGIN_CATALOG.find((entry) => entry.type === type)?.fields.map((field) => field.name))
        .not.toContain('dyeHeaders')
    }
  })
})
