import * as yaml from 'js-yaml'
import { mutationDocumentToYaml } from './resource-document'

export interface ConfigMapResource {
  apiVersion: 'v1'
  kind: 'ConfigMap'
  metadata: { name: string; namespace: string; labels?: Record<string, string>; annotations?: Record<string, string>; resourceVersion?: string }
  data?: Record<string, string>
  binaryData?: Record<string, string>
  immutable?: boolean
  [key: string]: unknown
}

export const createEmptyConfigMap = (): ConfigMapResource => ({
  apiVersion: 'v1', kind: 'ConfigMap', metadata: { name: '', namespace: 'default' }, data: {},
})

export const createConfigMapReplacement = (metadata: { name: string; namespace?: string; labels?: Record<string, string>; annotations?: Record<string, string>; resourceVersion?: string }): ConfigMapResource => ({
  apiVersion: 'v1', kind: 'ConfigMap', metadata: { name: metadata.name, namespace: metadata.namespace || 'default', labels: metadata.labels, annotations: metadata.annotations, resourceVersion: metadata.resourceVersion }, data: {},
})

export function configMapFromYaml(value: string): ConfigMapResource {
  const raw = yaml.load(value) as Partial<ConfigMapResource>
  if (!raw || raw.kind !== 'ConfigMap') throw new Error('YAML must contain a ConfigMap')
  return { ...raw, apiVersion: 'v1', kind: 'ConfigMap', metadata: { name: raw.metadata?.name || '', namespace: raw.metadata?.namespace || 'default', ...raw.metadata } }
}

export const configMapToYaml = (value: ConfigMapResource, mode: 'create' | 'update') =>
  mutationDocumentToYaml(value, 'configmap', mode)
