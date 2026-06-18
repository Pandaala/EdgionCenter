import * as yaml from 'js-yaml'
import { dumpYaml } from './yaml-utils'

export interface EdgionConfigDataResource {
  apiVersion: string
  kind: 'EdgionConfigData'
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

export function createEmpty(): EdgionConfigDataResource {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'EdgionConfigData',
    metadata: { name: '', namespace: 'default' },
    spec: {},
  }
}

export function normalize(raw: any): EdgionConfigDataResource {
  return {
    apiVersion: raw.apiVersion || 'edgion.io/v1',
    kind: 'EdgionConfigData',
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

export function toYaml(r: EdgionConfigDataResource): string {
  return dumpYaml(r)
}

export function fromYaml(yamlStr: string): EdgionConfigDataResource {
  return normalize(yaml.load(yamlStr) as any)
}
