import { validatePluginStages } from './plugin-document'
/**
 * EdgionPlugins 工具函数
 */

import * as yaml from 'js-yaml'
import { EDGION_PLUGINS_API_VERSION, EDGION_PLUGINS_KIND } from '@/types/edgion-plugins'
import type {
  AccessLogExternField,
  AccessLogExternSource,
  EdgionPlugins,
  EdgionPluginsSpec,
} from '@/types/edgion-plugins'
import { dumpYaml } from './yaml-utils'
import { mutationDocumentToYaml } from './resource-document'

/**
 * 默认的 EdgionPlugins YAML 模板
 */
export const DEFAULT_EDGION_PLUGINS_YAML = `apiVersion: edgion.io/v1
kind: EdgionPlugins
metadata:
  name: example-plugins
  namespace: default
spec:
  requestPlugins:
    - type: BasicAuth
      config:
        secretGroups:
          - name: users
            secretRefs:
              - name: basic-auth-users
                namespace: default
`

/**
 * 创建空的 EdgionPlugins 对象
 */
export function createEmptyEdgionPlugins(): EdgionPlugins {
  return {
    apiVersion: EDGION_PLUGINS_API_VERSION,
    kind: EDGION_PLUGINS_KIND,
    metadata: {
      name: '',
      namespace: 'default',
      labels: {},
      annotations: {},
    },
    spec: {},
  }
}

/**
 * Validate editor structure without inserting defaults or projecting fields.
 */
export function normalizeEdgionPlugins(resource: EdgionPlugins | Record<string, unknown>): EdgionPlugins {
  if (!resource || typeof resource !== 'object' || resource.kind !== EDGION_PLUGINS_KIND) {
    throw new Error('Expected an EdgionPlugins document')
  }
  if (!resource.metadata || typeof resource.metadata !== 'object' || Array.isArray(resource.metadata) || !resource.spec || typeof resource.spec !== 'object' || Array.isArray(resource.spec)) {
    throw new Error('EdgionPlugins metadata and spec are required')
  }
  validatePluginStages(resource.spec as Record<string, unknown>, ['requestPlugins', 'upstreamResponseFilterPlugins', 'upstreamResponseBodyFilterPlugins', 'upstreamResponsePlugins'])
  const waf = (resource.spec as Record<string, unknown>).waf
  if (waf != null) {
    if (typeof waf !== 'object' || Array.isArray(waf)) throw new Error('EdgionPlugins spec.waf must be an object')
    for (const key of ['policyRef', 'activeProfileRef', 'requestBody']) {
      const field = (waf as Record<string, unknown>)[key]
      if (field != null && (typeof field !== 'object' || Array.isArray(field))) {
        throw new Error(`EdgionPlugins spec.waf.${key} must be an object`)
      }
    }
  }
  return structuredClone(resource) as EdgionPlugins
}

/**
 * Serialize the complete editable document, including explicit empty values.
 */
export function edgionPluginsToYAML(resource: EdgionPlugins): string {
  return dumpYaml(resource)
}

export function edgionPluginsToMutationYAML(resource: EdgionPlugins, mode: 'create' | 'update'): string {
  const errors = [
    ...validateAccessLogExtern(resource.spec.accessLogExtern),
    ...validatePluginBodyRequirements(resource.spec),
  ]
  if (errors.length > 0) throw new Error(errors.join('; '))
  return mutationDocumentToYaml(resource, 'edgionplugins', mode)
}

/**
 * Parse YAML through the same identity and structural boundary as API views.
 */
export function yamlToEdgionPlugins(yamlStr: string): EdgionPlugins {
  const parsed = yaml.load(yamlStr)
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('EdgionPlugins YAML must contain an object')
  }
  return normalizeEdgionPlugins(parsed as Record<string, unknown>)
}

export function countPluginsByStage(spec: EdgionPluginsSpec | undefined) {
  return {
    waf: spec?.waf != null ? 1 : 0,
    request: spec?.requestPlugins?.length ?? 0,
    responseFilter: spec?.upstreamResponseFilterPlugins?.length ?? 0,
    responseBodyFilter: spec?.upstreamResponseBodyFilterPlugins?.length ?? 0,
    response: spec?.upstreamResponsePlugins?.length ?? 0,
  }
}

type BodyCapableStage = keyof Pick<
  EdgionPluginsSpec,
  'requestPlugins' | 'upstreamResponseFilterPlugins' | 'upstreamResponseBodyFilterPlugins' | 'upstreamResponsePlugins'
>

/**
 * Conservative browser capability gate for operator-configurable body blocks.
 * The Controller's EdgionPlugin::accepts_body remains authoritative. Source DSL
 * uses a UI heuristic; opaque bytecode stays permissive because the browser
 * cannot inspect its compiled builtin table.
 */
function objectValue(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
}

function conditionSetUsesRequestBody(value: unknown): boolean {
  const set = objectValue(value)
  const consumesBody = (leaf: unknown) => {
    const condition = objectValue(leaf)
    return condition.type === 'hmacAuth' && condition.validateRequestBody === true
  }
  if (Array.isArray(set.allOf)) return set.allOf.some(consumesBody)
  return Array.isArray(set.anyOf) && set.anyOf.some((clause) => {
    const group = objectValue(clause)
    return Array.isArray(group.allOf) ? group.allOf.some(consumesBody) : consumesBody(clause)
  })
}

export function pluginAcceptsBodyRequirement(
  stage: BodyCapableStage,
  type: string,
  config: Record<string, unknown> | undefined,
): boolean {
  if (stage !== 'requestPlugins') return false
  if (['Wasm', 'AiProxy', 'AiGuard', 'RequestBodyBuffer', 'JsonSchemaValidation', 'FormJsonTransform'].includes(type)) return true
  if (type === 'HmacAuth') return config?.validateRequestBody === true
  if (type === 'ForwardAuth') return config?.forwardBody === true
  if (type === 'ProxyRewrite') return config?.jsonBody != null
  if (type === 'ExtProc') return objectValue(config?.processingMode).requestBodyMode === 'BUFFERED'
  if (type === 'RequestRestriction') return conditionSetUsesRequestBody(config?.conditions)
  if (type === 'RequestAccessPolicy') {
    return Object.values(objectValue(config?.resolvedProfiles ?? config?.profiles)).some((profile) => {
      const groups = objectValue(profile).requiredRuleGroups
      return Array.isArray(groups) && groups.some((group) => {
        const rules = objectValue(group).anyOfRules
        return Array.isArray(rules) && rules.some((rule) => {
          const ruleConfig = objectValue(objectValue(rule).config)
          return conditionSetUsesRequestBody(ruleConfig.conditions) ||
            (Array.isArray(ruleConfig.resolvedCandidates) && ruleConfig.resolvedCandidates.some((candidate) => conditionSetUsesRequestBody(objectValue(candidate).conditions)))
        })
      })
    })
  }
  if (type === 'Dsl') {
    if (typeof config?.bytecode === 'string' && config.bytecode.trim() !== '') return true
    return typeof config?.source === 'string' && /\breq\s*\.\s*body\b/.test(config.source)
  }
  return false
}

export function validatePluginBodyRequirements(spec: EdgionPluginsSpec): string[] {
  const errors: string[] = []
  const stages: BodyCapableStage[] = [
    'requestPlugins',
    'upstreamResponseFilterPlugins',
    'upstreamResponseBodyFilterPlugins',
    'upstreamResponsePlugins',
  ]
  for (const stage of stages) {
    for (const [index, entry] of (spec[stage] ?? []).entries()) {
      if (entry.enable === false) continue
      if ('body' in entry && entry.body !== undefined &&
          !pluginAcceptsBodyRequirement(stage, entry.type, entry.config)) {
        errors.push(`${stage}[${index}]: plugin ${entry.type} does not accept an operator body requirement`)
      }
    }
  }
  return errors
}

const ACCESS_LOG_EXTERN_MAX_FIELDS = 16
const ACCESS_LOG_EXTERN_MAX_KEY_BYTES = 63
const BLOCKED_HEADER_NAMES = new Set([
  'authorization',
  'proxy-authorization',
  'cookie',
  'set-cookie',
  'x-api-key',
  'api-key',
  'apikey',
  'x-auth-token',
  'authentication',
  'x-amz-security-token',
  'www-authenticate',
  'proxy-authenticate',
])
const BLOCKED_CTX_KEYS = new Set(['jwt_claims', 'oidc_claims', 'jwe_payload'])
const BLOCKED_ANNOTATION_PREFIXES = ['kubectl.kubernetes.io/', 'edgion.io/']
const ACCESS_LOG_EXTERN_SOURCES = new Set<AccessLogExternSource>([
  'routeLabel', 'routeAnnotation', 'header', 'query', 'cookie', 'respHeader', 'ctx',
])

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length
}

function isBlockedAccessLogName(source: AccessLogExternSource, name: string): boolean {
  if (source === 'header' || source === 'respHeader') {
    return BLOCKED_HEADER_NAMES.has(name.toLocaleLowerCase('en-US'))
  }
  if (source === 'ctx') return BLOCKED_CTX_KEYS.has(name)
  if (source === 'routeAnnotation') {
    return BLOCKED_ANNOTATION_PREFIXES.some((prefix) => name.startsWith(prefix))
  }
  return false
}

/**
 * Mirror the Edgion schema-layer validation for spec.accessLogExtern.
 * Error indexes intentionally match the operator document array indexes.
 */
export function validateAccessLogExtern(fields: readonly AccessLogExternField[] | undefined): string[] {
  if (!fields) return []

  const errors: string[] = []
  if (fields.length > ACCESS_LOG_EXTERN_MAX_FIELDS) {
    errors.push(`accessLogExtern: at most ${ACCESS_LOG_EXTERN_MAX_FIELDS} fields per resource (got ${fields.length})`)
  }

  const seen = new Set<string>()
  fields.forEach((field, index) => {
    if (!field || typeof field !== 'object' || Array.isArray(field)) {
      errors.push(`accessLogExtern[${index}]: field must be an object`)
      return
    }
    let invalid = false
    if (typeof field.key !== 'string') {
      errors.push(`accessLogExtern[${index}]: key must be a string`)
      invalid = true
    } else if (field.key.length === 0) {
      errors.push(`accessLogExtern[${index}]: key must not be empty`)
      invalid = true
    } else if (utf8ByteLength(field.key) > ACCESS_LOG_EXTERN_MAX_KEY_BYTES) {
      errors.push(`accessLogExtern[${index}]: key must be at most ${ACCESS_LOG_EXTERN_MAX_KEY_BYTES} bytes`)
      invalid = true
    }
    if (typeof field.from !== 'string' || !ACCESS_LOG_EXTERN_SOURCES.has(field.from as AccessLogExternSource)) {
      errors.push(`accessLogExtern[${index}]: from must be one of routeLabel, routeAnnotation, header, query, cookie, respHeader, ctx`)
      invalid = true
    }
    if (typeof field.name !== 'string') {
      errors.push(`accessLogExtern[${index}]: name must be a string`)
      invalid = true
    } else if (field.name.length === 0) {
      errors.push(`accessLogExtern[${index}]: name must not be empty`)
      invalid = true
    } else if (utf8ByteLength(field.name) > ACCESS_LOG_EXTERN_MAX_KEY_BYTES) {
      errors.push(`accessLogExtern[${index}]: name must be at most ${ACCESS_LOG_EXTERN_MAX_KEY_BYTES} bytes`)
      invalid = true
    } else if (ACCESS_LOG_EXTERN_SOURCES.has(field.from as AccessLogExternSource) &&
               isBlockedAccessLogName(field.from as AccessLogExternSource, field.name)) {
      errors.push(`accessLogExtern[${index}]: name '${field.name}' is blocked for this source and can never be logged`)
      invalid = true
    }
    if (invalid) return
    if (seen.has(field.key)) {
      errors.push(`accessLogExtern[${index}]: duplicate key '${field.key}'`)
    } else {
      seen.add(field.key)
    }
  })
  return errors
}
