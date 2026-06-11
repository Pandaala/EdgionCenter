import * as yaml from 'js-yaml'
import { dumpYaml } from './yaml-utils'

export interface PluginMetaDataResource {
  apiVersion: string
  kind: 'PluginMetaData'
  metadata: {
    name: string
    namespace?: string
    labels?: Record<string, string>
    annotations?: Record<string, string>
    resourceVersion?: string
    creationTimestamp?: string
  }
  spec: {
    description?: string
    schema?: any
    defaultConfig?: any
  }
}

export function createEmpty(): PluginMetaDataResource {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'PluginMetaData',
    metadata: { name: '', namespace: 'default' },
    spec: {},
  }
}

export function normalize(raw: any): PluginMetaDataResource {
  return {
    apiVersion: raw.apiVersion || 'edgion.io/v1',
    kind: 'PluginMetaData',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: {
      description: raw.spec?.description,
      schema: raw.spec?.schema,
      defaultConfig: raw.spec?.defaultConfig,
    },
  }
}

export function toYaml(r: PluginMetaDataResource): string {
  return dumpYaml(r)
}

export function fromYaml(yamlStr: string): PluginMetaDataResource {
  return normalize(yaml.load(yamlStr) as any)
}
