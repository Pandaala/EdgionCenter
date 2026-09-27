import { describe, expect, it } from 'vitest'
import { PLUGIN_TYPES, STAGE_PLUGIN_TYPES } from '@/types/edgion-plugins'
import {
  HTTP_PLUGIN_CATALOG,
  pluginTypesForStage,
} from './pluginCatalog'

describe('dashboard HTTP plugin catalog', () => {
  it('matches current flattened auth, restriction, transformation and processing fields', () => {
    const fields = (type: string) => HTTP_PLUGIN_CATALOG.find((entry) => entry.type === type)!.fields
    const names = (type: string) => fields(type).map((field) => field.name)
    expect(names('ForwardAuth')).toEqual(expect.arrayContaining(['target', 'tls', 'timeoutMs', 'request', 'decision', 'forwardBody']))
    expect(names('ForwardAuth')).not.toContain('conn')
    expect(names('RequestRestriction')).toContain('conditions')
    expect(names('RequestRestriction')).not.toContain('ruleGroups')
    expect(fields('IpRestriction').find((field) => field.name === 'ipSource')?.options).toEqual(['RemoteIp', 'DirectPeerIp'])
    expect(names('ProxyRewrite')).toContain('jsonBody')
    expect(names('Dsl')).toEqual(expect.arrayContaining(['maxBodyScanPasses', 'httpMaxRequestBodyBytes', 'rateLimitPolicies']))
    expect(names('ExtProc')).toEqual(expect.arrayContaining(['disableImmediateResponse', 'allowModeOverride', 'allowedOverrideModes']))
    expect(names('LdapAuth')).not.toContain('cacheTtl')
    expect(names('LdapAuth')).not.toContain('lruCache')
  })

  it('exposes the current request-only guard, credential, body and webhook plugins', () => {
    for (const type of ['AiGuard', 'CredentialInjector', 'RequestBodyBuffer', 'JsonSchemaValidation', 'FormJsonTransform', 'GeoIpLocation', 'Guardrail', 'WebhookKeyGet']) {
      expect(HTTP_PLUGIN_CATALOG.find((entry) => entry.type === type)?.stages).toEqual(['requestPlugins'])
    }
    const fields = (type: string) => HTTP_PLUGIN_CATALOG.find((entry) => entry.type === type)!.fields
    expect(fields('CredentialInjector').map((field) => field.name)).toEqual(['credentialRef', 'target'])
    expect(fields('FormJsonTransform')[0].options).toEqual(['formToJson', 'jsonToForm'])
    for (const forbidden of ['retry', 'timeoutMsTemplate', 'success', 'resolvedSecrets']) {
      expect(fields('Guardrail').map((field) => field.name)).not.toContain(forbidden)
    }
    expect(fields('AiGuard').find((field) => field.name === 'rejectionMode')?.options).toEqual(['error', 'contentFilter'])
  })

  it('offers current access-policy profiles without obsolete bypass fields', () => {
    expect(PLUGIN_TYPES).not.toContain('GlobalAccessControl')
    const definition = HTTP_PLUGIN_CATALOG.find((entry) => entry.type === 'RequestAccessPolicy')!
    expect(definition.stages).toEqual(['requestPlugins'])
    expect(definition.fields.map((field) => field.name)).toEqual([
      'defaultProfile', 'description', 'message', 'profiles', 'activeProfileRef', 'status',
    ])
  })

  it('offers RequestId correlation rules instead of the removed TraceContext config', () => {
    expect(PLUGIN_TYPES).not.toContain('TraceContext')
    const definition = HTTP_PLUGIN_CATALOG.find((entry) => entry.type === 'RequestId')!
    expect(definition.stages).toEqual(['requestPlugins'])
    expect(definition.fields.map((field) => field.name)).toEqual(['defaultId', 'ids'])
  })

  it('uses the current local limiter name and key-miss policy fields', () => {
    expect(PLUGIN_TYPES).not.toContain('RateLimit')
    for (const type of ['RateLimitLocal', 'RateLimitRedis']) {
      const definition = HTTP_PLUGIN_CATALOG.find((entry) => entry.type === type)!
      expect(definition.stages).toEqual(['requestPlugins'])
      expect(definition.fields.map((field) => field.name)).not.toContain('onMissingKey')
      expect(definition.fields.find((field) => field.name === 'onKeyMiss')?.options).toEqual(['Allow', 'Deny'])
    }
    const local = HTTP_PLUGIN_CATALOG.find((entry) => entry.type === 'RateLimitLocal')!
    expect(local.fields.find((field) => field.name === 'onLimitExceeded')?.options).toEqual(['Reject', 'Continue'])
  })

  it('exposes the current AiProxy fields only in the request stage', () => {
    const definition = HTTP_PLUGIN_CATALOG.find((entry) => entry.type === 'AiProxy')!
    expect(definition.stages).toEqual(['requestPlugins'])
    expect(definition.fields.map((field) => field.name)).toEqual([
      'backendSelection', 'injectUsageTracking', 'onUnknownModel', 'modelRoutes', 'tokenQuota',
    ])
    expect(definition.fields.find((field) => field.name === 'backendSelection')?.defaultValue).toEqual({ type: 'Weighted' })
  })

  it('keeps the 48 registered plugin definitions and type choices consistent', () => {
    expect(HTTP_PLUGIN_CATALOG).toHaveLength(48)
    expect(new Set(HTTP_PLUGIN_CATALOG.map((entry) => entry.type)).size).toBe(48)
    expect(PLUGIN_TYPES).toHaveLength(48)
    expect([...PLUGIN_TYPES]).toEqual(HTTP_PLUGIN_CATALOG.map((entry) => entry.type))
    expect(PLUGIN_TYPES).not.toContain('ExtensionRef')
  })

  it('keeps stage choices aligned with the catalog', () => {
    expect(STAGE_PLUGIN_TYPES.requestPlugins).toEqual(pluginTypesForStage('requestPlugins'))
    expect(STAGE_PLUGIN_TYPES.upstreamResponseFilterPlugins).toEqual(pluginTypesForStage('upstreamResponseFilterPlugins'))
    expect(STAGE_PLUGIN_TYPES.upstreamResponseBodyFilterPlugins).toEqual(pluginTypesForStage('upstreamResponseBodyFilterPlugins'))
    expect(STAGE_PLUGIN_TYPES.upstreamResponsePlugins).toEqual(pluginTypesForStage('upstreamResponsePlugins'))
    expect(pluginTypesForStage('upstreamResponsePlugins')).toEqual([])
    expect(pluginTypesForStage('upstreamResponseBodyFilterPlugins')).toEqual(['BandwidthLimit', 'Wasm'])
    expect(pluginTypesForStage('upstreamResponseFilterPlugins')).toEqual([
      'ResponseHeaderModifier', 'DebugAccessLogToHeader', 'ResponseRewrite', 'Dsl', 'Wasm',
    ])
    expect(pluginTypesForStage('requestPlugins')).toHaveLength(44)
  })

  it('publishes a structured field catalog for every non-empty Rust config', () => {
    const fieldless = HTTP_PLUGIN_CATALOG.filter((entry) => entry.fields.length === 0).map((entry) => entry.type)
    expect(fieldless).toEqual(['RequestBodyBuffer', 'DebugAccessLogToHeader'])
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
