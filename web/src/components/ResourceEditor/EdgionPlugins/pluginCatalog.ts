import type { EdgionPluginsSpec } from '@/types/edgion-plugins'

export type PluginStage = keyof Pick<EdgionPluginsSpec,
  'requestPlugins' | 'upstreamResponseFilterPlugins' | 'upstreamResponseBodyFilterPlugins' | 'upstreamResponsePlugins'>

export type FieldKind = 'string' | 'number' | 'boolean' | 'object' | 'array' | 'code'

export interface PluginField {
  name: string
  kind: FieldKind
  defaultValue?: unknown
  options?: readonly string[]
}

export interface PluginDefinition {
  type: string
  fields: readonly PluginField[]
  stages: readonly PluginStage[]
}

const f = (name: string, kind: FieldKind, defaultValue?: unknown, options?: readonly string[]): PluginField => ({
  name, kind, defaultValue, options,
})
const strings = (...names: string[]) => names.map((name) => f(name, 'string', ''))
const numbers = (...names: string[]) => names.map((name) => f(name, 'number', 0))
const booleans = (...names: string[]) => names.map((name) => f(name, 'boolean', false))
const objects = (...names: string[]) => names.map((name) => f(name, 'object', {}))
const arrays = (...names: string[]) => names.map((name) => f(name, 'array', []))

const REQUEST: readonly PluginStage[] = ['requestPlugins']
const RESPONSE_FILTER: readonly PluginStage[] = ['upstreamResponseFilterPlugins']

export const HTTP_PLUGIN_CATALOG: readonly PluginDefinition[] = [
  { type: 'RequestHeaderModifier', stages: REQUEST, fields: [...arrays('set', 'add', 'remove')] },
  { type: 'ResponseHeaderModifier', stages: RESPONSE_FILTER, fields: [...arrays('set', 'add', 'remove')] },
  { type: 'RequestRedirect', stages: REQUEST, fields: [...strings('scheme', 'hostname'), ...numbers('port', 'statusCode'), ...objects('path')] },
  { type: 'UrlRewrite', stages: REQUEST, fields: [...strings('hostname'), ...objects('path')] },
  {
    type: 'RequestMirror',
    stages: REQUEST,
    fields: [
      ...objects('backendRef', 'fraction'),
      ...numbers(
        'percentage',
        'connectTimeoutMs',
        'writeTimeoutMs',
        'maxBufferedChunks',
        'maxConcurrent',
        'channelFullTimeoutMs',
      ),
      ...booleans('mirrorLog'),
    ],
  },
  { type: 'BasicAuth', stages: REQUEST, fields: [...arrays('secretGroups'), ...booleans('hideCredentials'), ...numbers('authFailureDelayMs'), ...strings('realm', 'anonymous')] },
  { type: 'Cors', stages: REQUEST, fields: [...strings('allowOrigins', 'allowMethods', 'allowHeaders', 'exposeHeaders', 'timingAllowOrigins'), ...arrays('allowOriginsByRegex', 'timingAllowOriginsByRegex'), ...booleans('allowCredentials', 'preflightContinue', 'allowPrivateNetwork'), ...numbers('maxAge')] },
  { type: 'Csrf', stages: REQUEST, fields: [...objects('secretRef'), ...numbers('expires'), ...strings('name'), ...booleans('cookieSecure')] },
  { type: 'IpRestriction', stages: REQUEST, fields: [...arrays('allow', 'deny', 'allowRefs', 'denyRefs'), f('ipSource', 'string', 'remoteAddr', ['clientIp', 'remoteAddr']), ...strings('message'), ...numbers('status'), f('defaultAction', 'string', 'deny', ['allow', 'deny'])] },
  { type: 'JwtAuth', stages: REQUEST, fields: [...strings('algorithm', 'header', 'query', 'cookie', 'anonymous', 'keyClaimName', 'realm'), ...booleans('allowTokenInQuery', 'hideCredentials', 'base64Secret', 'storeClaimsInCtx'), ...numbers('lifetimeGracePeriod', 'maximumExpiration', 'authFailureDelayMs'), ...arrays('audiences', 'issuerGroups'), ...objects('claimsToHeaders')] },
  { type: 'JweDecrypt', stages: REQUEST, fields: [...objects('secretRef', 'payloadToHeaders'), ...strings('keyManagementAlgorithm', 'contentEncryptionAlgorithm', 'header', 'forwardHeader', 'stripPrefix'), ...booleans('strict', 'hideCredentials', 'base64Secret', 'storePayloadInCtx'), ...numbers('maxTokenSize', 'authFailureDelayMs', 'maxHeaderValueBytes', 'maxTotalHeaderBytes'), ...arrays('allowedAlgorithms')] },
  { type: 'HmacAuth', stages: REQUEST, fields: [...arrays('secretGroups', 'algorithms', 'enforceHeaders', 'upstreamHeaderFields'), ...numbers('clockSkew', 'authFailureDelayMs', 'minSecretLength'), ...booleans('validateRequestBody', 'hideCredentials'), ...strings('anonymous', 'realm', 'secretField', 'usernameField')] },
  { type: 'HeaderCertAuth', stages: REQUEST, fields: [...strings('mode', 'certificateHeaderName', 'certificateHeaderFormat', 'consumerBy', 'errorMessage'), ...booleans('hideCredentials', 'skipConsumerLookup', 'allowAnonymous'), ...arrays('caSecretRefs'), ...objects('upstreamHeaders'), ...numbers('errorStatus', 'authFailureDelayMs')] },
  { type: 'KeyAuth', stages: REQUEST, fields: [...arrays('keySources', 'secretGroups', 'upstreamHeaderFields'), ...booleans('hideCredentials'), ...numbers('authFailureDelayMs'), ...strings('anonymous', 'realm', 'keyField')] },
  { type: 'LdapAuth', stages: REQUEST, fields: [...strings('ldapHost', 'baseDn', 'attribute', 'bindDnTemplate', 'headerType', 'anonymous', 'realm', 'credentialIdentifierHeader', 'anonymousHeader'), ...numbers('ldapPort', 'authFailureDelayMs', 'cacheTtl', 'timeout', 'keepalive'), ...booleans('ldaps', 'startTls', 'hideCredentials'), ...objects('tls', 'lruCache')] },
  { type: 'Mock', stages: REQUEST, fields: [...numbers('statusCode', 'delay'), ...strings('body', 'contentType'), ...objects('headers'), ...booleans('terminate')] },
  { type: 'FaultInjection', stages: REQUEST, fields: [...objects('delay', 'abort')] },
  { type: 'DebugAccessLogToHeader', stages: RESPONSE_FILTER, fields: [] },
  { type: 'ProxyRewrite', stages: REQUEST, fields: [...strings('uri', 'host', 'method'), ...objects('regexUri', 'headers')] },
  { type: 'RequestRestriction', stages: REQUEST, fields: [...strings('mode', 'message'), ...arrays('ruleGroups'), ...numbers('status')] },
  { type: 'ResponseRewrite', stages: RESPONSE_FILTER, fields: [...numbers('statusCode'), ...objects('headers')] },
  { type: 'RateLimit', stages: REQUEST, fields: [...numbers('rate', 'rejectStatus', 'skewTolerance', 'estimatorSlotsK'), ...strings('interval', 'onMissingKey', 'defaultKey', 'rejectMessage', 'scope'), ...arrays('key'), ...booleans('showLimitHeaders'), ...objects('headerNames')] },
  { type: 'RateLimitRedis', stages: REQUEST, fields: [...strings('redisRef', 'onMissingKey', 'defaultKey', 'rejectMessage', 'keyPrefix', 'onRedisFailure', 'requestTimeout'), ...arrays('policies', 'key'), ...numbers('rejectStatus', 'cost', 'maxKeyLen'), ...booleans('showLimitHeaders'), ...objects('headerNames')] },
  { type: 'CtxSet', stages: REQUEST, fields: [...arrays('vars')] },
  { type: 'RealIp', stages: REQUEST, fields: [...arrays('trustedIps'), ...strings('realIpHeader'), ...booleans('recursive'), ...numbers('maxTrustedHops')] },
  { type: 'ForwardAuth', stages: REQUEST, fields: [...objects('conn', 'request', 'decision')] },
  { type: 'OpenidConnect', stages: REQUEST, fields: [
    ...strings('discovery', 'clientId', 'realm', 'scope', 'redirectUri', 'postLogoutRedirectUri', 'logoutPath', 'sessionCookieName', 'sessionCookieSameSite', 'verificationMode', 'tokenSigningAlg', 'introspectionEndpoint', 'introspectionEndpointAuthMethod', 'tokenEndpointAuthMethod'),
    ...objects('clientSecretRef', 'sessionSecretRef', 'authorizationParams', 'introspection', 'accessToken', 'tls', 'claimsToHeaders'),
    ...arrays('requiredScopes', 'allowedSigningAlgs', 'issuers', 'audiences'),
    ...booleans('bearerOnly', 'usePkce', 'useNonce', 'revokeTokensOnLogout', 'sessionCookieHttpOnly', 'sessionCookieSecure', 'renewAccessTokenOnExpiry', 'useJwks', 'setAccessTokenHeader', 'setIdTokenHeader', 'setUserinfoHeader', 'accessTokenInAuthorizationHeader', 'storeClaimsInCtx', 'hideCredentials'),
    ...numbers('sessionLifetime', 'maxSessionCookieBytes', 'accessTokenExpiresLeeway', 'clockSkewSeconds', 'jwksCacheTtl', 'jwksMinRefreshInterval', 'introspectionCacheTtl', 'maxHeaderValueBytes', 'maxTotalHeaderBytes', 'timeout', 'discoveryMaxResponseBytes', 'jwksMaxResponseBytes', 'userinfoMaxResponseBytes', 'authFailureDelayMs'),
  ] },
  { type: 'BandwidthLimit', stages: ['upstreamResponseBodyFilterPlugins'], fields: [...strings('rate')] },
  { type: 'DirectEndpoint', stages: REQUEST, fields: [...objects('from', 'extract', 'dyeHeaders'), ...numbers('port'), ...strings('onMissing', 'onInvalid'), ...booleans('inheritTls')] },
  { type: 'AllEndpointStatus', stages: REQUEST, fields: [...numbers('timeoutMs', 'wallTimeoutMs', 'maxEndpoints', 'maxBodySize', 'concurrencyLimit'), ...booleans('includeResponseHeaders'), ...strings('methodOverride', 'pathOverride')] },
  { type: 'DynamicInternalUpstream', stages: REQUEST, fields: [...objects('from', 'extract', 'dyeHeaders'), ...arrays('rules'), ...strings('onMissing', 'onNoMatch', 'onInvalid')] },
  { type: 'DynamicExternalUpstream', stages: REQUEST, fields: [...objects('from', 'extract', 'domainMap', 'dyeHeaders'), ...strings('onMissing', 'onNoMatch')] },
  { type: 'Dsl', stages: ['requestPlugins', 'upstreamResponseFilterPlugins'], fields: [...strings('name'), f('source', 'code', ''), f('bytecode', 'code', ''), ...numbers('maxSteps', 'maxLoopIterations', 'maxCallCount', 'maxStackDepth', 'maxStringLen', 'maxListLen', 'maxMapSize', 'httpMaxTimeoutMs', 'httpMaxResponseBodyBytes'), ...strings('errorPolicy'), ...booleans('httpBlockPrivateIps'), ...arrays('linksysRefs')] },
  { type: 'RegionRoute', stages: REQUEST, fields: [...strings('myRegion'), ...arrays('keyGet', 'hashKeyGet', 'routeRules', 'regions'), ...objects('hashCalc', 'routeByKeyConfMatch', 'overrideRef', 'dyeHeaders')] },
  { type: 'TraceContext', stages: REQUEST, fields: [...booleans('generateWhenMissing', 'echoToClient', 'trustInbound', 'defaultSampled')] },
  { type: 'ExtProc', stages: ['requestPlugins', 'upstreamResponsePlugins'], fields: [...objects('grpcService', 'processingMode'), ...booleans('failureModeAllow')] },
  { type: 'GlobalAccessControl', stages: REQUEST, fields: [...booleans('enable'), ...strings('activeProfile', 'description', 'message', 'ipSource'), ...objects('profiles', 'activeProfileRef'), ...numbers('status')] },
  { type: 'Canary', stages: REQUEST, fields: [...booleans('enable'), ...strings('activeProfile', 'onInvalid'), ...objects('profiles', 'activeProfileRef', 'dyeHeaders')] },
  { type: 'Wasm', stages: ['requestPlugins', 'upstreamResponseFilterPlugins', 'upstreamResponseBodyFilterPlugins'], fields: [...objects('source'), ...strings('sha256'), f('pluginConfig', 'code', ''), f('vmConfig', 'code', ''), ...booleans('failOpen'), ...numbers('timeoutMs', 'instancePoolSize', 'calloutTimeoutMs'), ...arrays('calloutAllowlist')] },
] as const

export const PLUGIN_DEFINITION_BY_TYPE = new Map(HTTP_PLUGIN_CATALOG.map((definition) => [definition.type, definition]))

export function pluginTypesForStage(stage: PluginStage): string[] {
  return HTTP_PLUGIN_CATALOG.filter((definition) => definition.stages.includes(stage)).map((definition) => definition.type)
}

export function defaultConfigForPlugin(type: string): Record<string, unknown> {
  if (!PLUGIN_DEFINITION_BY_TYPE.has(type)) throw new Error(`Unknown plugin type: ${type}`)
  // Field controls display their typed empty state without materializing it.
  // Only an explicit edit adds a field to the operator document.
  return {}
}
