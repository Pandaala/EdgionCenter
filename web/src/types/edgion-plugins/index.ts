/**
 * EdgionPlugins 资源类型定义
 * 对应 Edgion 网关的 EdgionPlugins CRD (edgion.io/v1)
 */

import type { K8sObjectMeta, Condition } from '@/types/gateway-api'

/**
 * 插件条件 - 用于条件化插件执行
 * 支持 skip（跳过条件）和 run（运行条件）
 */
export interface PluginConditions {
  skip?: PluginConditionItem[]
  run?: PluginConditionItem[]
}

export type PluginConditionItem = Record<string, unknown>

export type DyeOutcome = 'success' | 'fallback' | 'reject' | 'fail-open' | 'fail-close'

export interface DyeRule {
  name: string
  on?: DyeOutcome[]
  value?: string
  conditions?: PluginConditions
}

export interface DyeSuite {
  request?: DyeRule[]
  response?: DyeRule[]
}

export interface BodyRequirement {
  maxBodySize?: string
  conditions?: PluginConditions
  onReadFailure?: 'failClose' | 'failOpen'
}

export type AccessLogExternSource =
  | 'routeLabel'
  | 'routeAnnotation'
  | 'header'
  | 'query'
  | 'cookie'
  | 'respHeader'
  | 'ctx'

export interface AccessLogExternField {
  key: string
  from: AccessLogExternSource
  name: string
}

/**
 * 插件入口 - 包含 enable/conditions + 扁平化的 type + config
 *
 * Rust serde 序列化格式（flatten + tag/content）：
 * ```yaml
 * - enable: false   # 仅 false 时序列化
 *   conditions: ... # 可选
 *   type: BasicAuth # EdgionPlugin 枚举 tag
 *   config:         # EdgionPlugin 枚举 content
 *     ...
 * ```
 */
export interface PluginEntry {
  /** 是否启用，默认 true（false 时才序列化） */
  enable?: boolean
  /** 条件化执行配置 */
  conditions?: PluginConditions
  /** Optional access-log label; must match [a-zA-Z0-9._-]{1,32}. */
  alias?: string
  /** Entry-level traffic tagging rules. */
  dye?: DyeSuite
  /** 插件类型名称，对应 EdgionPlugin 枚举变体 */
  type: string
  /** 插件配置，具体结构由 type 决定 */
  config: Record<string, unknown>
}

/** 请求阶段插件入口 */
export interface RequestFilterEntry extends PluginEntry {
  body?: BodyRequirement
}

/** 上游响应过滤阶段插件入口（同步） */
export type UpstreamResponseFilterEntry = PluginEntry

/** 上游响应体过滤阶段插件入口（同步，用于带宽限制等） */
export type UpstreamResponseBodyFilterEntry = PluginEntry

/** 上游响应阶段插件入口（异步） */
export interface UpstreamResponseEntry extends PluginEntry {
  body?: BodyRequirement
}

/**
 * EdgionPlugins Spec
 * 包含四个执行阶段的插件列表
 */
export interface EdgionPluginsSpec {
  /** 请求阶段插件（异步） */
  requestPlugins?: RequestFilterEntry[]
  /** 上游响应过滤阶段插件（同步） */
  upstreamResponseFilterPlugins?: UpstreamResponseFilterEntry[]
  /** 上游响应体过滤阶段插件（同步，带宽控制） */
  upstreamResponseBodyFilterPlugins?: UpstreamResponseBodyFilterEntry[]
  /** 上游响应阶段插件（异步） */
  upstreamResponsePlugins?: UpstreamResponseEntry[]
  /** Access-log projection declarations; this is not an execution stage. */
  accessLogExtern?: AccessLogExternField[]
  [key: string]: unknown
}

/**
 * EdgionPlugins Status
 */
export interface EdgionPluginsStatus {
  conditions?: Condition[]
}

export const EDGION_PLUGINS_API_VERSION = 'edgion.io/v1'
export const EDGION_PLUGINS_KIND = 'EdgionPlugins'

/**
 * EdgionPlugins 完整资源
 * apiVersion: edgion.io/v1
 * kind: EdgionPlugins
 * plural: edgionplugins
 * namespaced: true
 */
export interface EdgionPlugins {
  apiVersion: string
  kind: 'EdgionPlugins'
  metadata: K8sObjectMeta
  spec: EdgionPluginsSpec
  status?: EdgionPluginsStatus
  [key: string]: unknown
}

/**
 * 所有支持的插件类型名称
 * 对应 Edgion 中的 EdgionPlugin 枚举变体
 */
export const PLUGIN_TYPES = [
  // Gateway API 标准插件
  'RequestHeaderModifier',
  'ResponseHeaderModifier',
  'RequestRedirect',
  'UrlRewrite',
  'RequestMirror',
  // Edgion 自定义插件
  'BasicAuth',
  'Cors',
  'Csrf',
  'IpRestriction',
  'JwtAuth',
  'JweDecrypt',
  'HmacAuth',
  'HeaderCertAuth',
  'KeyAuth',
  'LdapAuth',
  'Mock',
  'FaultInjection',
  'DebugAccessLogToHeader',
  'ProxyRewrite',
  'RequestRestriction',
  'ResponseRewrite',
  'RateLimit',
  'RateLimitRedis',
  'CtxSet',
  'RealIp',
  'ForwardAuth',
  'OpenidConnect',
  'BandwidthLimit',
  'DirectEndpoint',
  'AllEndpointStatus',
  'DynamicInternalUpstream',
  'DynamicExternalUpstream',
  'Dsl',
  'RegionRoute',
  'TraceContext',
  'ExtProc',
  'GlobalAccessControl',
  'Canary',
  'Wasm',
] as const

export type PluginType = typeof PLUGIN_TYPES[number]

/** 各阶段支持的插件类型（参考 Edgion entry.rs 的注释） */
export const STAGE_PLUGIN_TYPES = {
  requestPlugins: [
    'RequestHeaderModifier', 'RequestRedirect', 'UrlRewrite', 'RequestMirror',
    'BasicAuth', 'Cors', 'Csrf', 'IpRestriction', 'JwtAuth', 'JweDecrypt',
    'HmacAuth', 'HeaderCertAuth', 'KeyAuth', 'LdapAuth', 'Mock', 'FaultInjection',
    'ProxyRewrite', 'RequestRestriction',
    'RateLimit', 'RateLimitRedis', 'CtxSet', 'RealIp', 'ForwardAuth',
    'OpenidConnect', 'DirectEndpoint', 'AllEndpointStatus',
    'DynamicInternalUpstream', 'DynamicExternalUpstream', 'Dsl',
    'RegionRoute', 'TraceContext', 'ExtProc', 'GlobalAccessControl', 'Canary', 'Wasm',
  ],
  upstreamResponseFilterPlugins: [
    'ResponseHeaderModifier', 'DebugAccessLogToHeader', 'ResponseRewrite', 'Dsl', 'Wasm',
  ],
  upstreamResponseBodyFilterPlugins: [
    'BandwidthLimit', 'Wasm',
  ],
  upstreamResponsePlugins: ['ExtProc'],
} as const
