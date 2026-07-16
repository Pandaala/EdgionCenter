import * as yaml from 'js-yaml'
import { buildMutationDocument, withCreateDefaults } from './resource-document'
import { dumpYaml } from './yaml-utils'

export type ConfigDataVisibility = 'Namespace' | 'Cluster'
export type ConfigDataType = 'KeyList' | 'IpList' | 'Selector' | 'RegionRouteOverride' | 'Misc'
export type JsonObject = Record<string, unknown>

export interface ConfigDataEntry {
  type: ConfigDataType
  config: JsonObject
  [key: string]: unknown
}

export interface EdgionConfigDataResource {
  apiVersion: string
  kind: 'EdgionConfigData'
  metadata: {
    name: string
    namespace?: string
    labels?: Record<string, string>
    annotations?: Record<string, string>
    [key: string]: unknown
  }
  spec: {
    enable: boolean
    active?: string
    visibility: ConfigDataVisibility
    data: ConfigDataEntry
    [key: string]: unknown
  }
  status?: unknown
  [key: string]: unknown
}

const CREATE_DEFAULTS: EdgionConfigDataResource = {
  apiVersion: 'edgion.io/v1',
  kind: 'EdgionConfigData',
  metadata: { name: '', namespace: 'default' },
  spec: {
    enable: true,
    visibility: 'Namespace',
    data: { type: 'Misc', config: {} },
  },
}

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

export function createEmpty(): EdgionConfigDataResource {
  return withCreateDefaults(undefined, CREATE_DEFAULTS)
}

/** Validate the resource identity and retain the complete API view unchanged. */
export function normalize(raw: unknown): EdgionConfigDataResource {
  if (!isObject(raw) || raw.kind !== 'EdgionConfigData' || !isObject(raw.metadata) || !isObject(raw.spec)) {
    throw new Error('YAML must contain an EdgionConfigData resource with metadata and spec')
  }
  if (!isObject(raw.spec.data)
    || !['KeyList', 'IpList', 'Selector', 'RegionRouteOverride', 'Misc'].includes(String(raw.spec.data.type))
    || !isObject(raw.spec.data.config)) {
    throw new Error('EdgionConfigData spec.data must contain type and config')
  }
  return raw as unknown as EdgionConfigDataResource
}

export function toMutationDocument(
  resource: EdgionConfigDataResource,
  mode: 'create' | 'update',
): Record<string, unknown> {
  return buildMutationDocument(resource, { resourceKind: 'edgionconfigdata', mode })
}

/** Serialize the complete API view for the YAML tab. */
export function toYaml(resource: EdgionConfigDataResource): string {
  return dumpYaml(resource)
}

/** Serialize only the operator-owned document accepted by create/update. */
export function toMutationYaml(resource: EdgionConfigDataResource, mode: 'create' | 'update'): string {
  return dumpYaml(toMutationDocument(resource, mode))
}

export function fromYaml(yamlStr: string): EdgionConfigDataResource {
  return normalize(yaml.load(yamlStr))
}

export function parseConfigYaml(yamlStr: string): JsonObject {
  const parsed = yaml.load(yamlStr)
  if (!isObject(parsed)) throw new Error('Config payload must be a YAML object')
  return parsed
}

export function configToYaml(config: JsonObject): string {
  return dumpYaml(config)
}

export function replaceConfigDataType(
  resource: EdgionConfigDataResource,
  type: ConfigDataType,
): EdgionConfigDataResource {
  if (resource.spec.data.type === type) return resource
  return {
    ...resource,
    spec: {
      ...resource.spec,
      data: {
        ...resource.spec.data,
        type,
        config: type === 'KeyList' ? { matchMode: 'exact', items: [] }
          : type === 'IpList' ? { items: [] }
          : {},
      },
    },
  }
}
