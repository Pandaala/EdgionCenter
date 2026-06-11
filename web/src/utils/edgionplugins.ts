/**
 * EdgionPlugins 工具函数
 */

import * as yaml from 'js-yaml'
import { EDGION_PLUGINS_API_VERSION, EDGION_PLUGINS_KIND } from '@/types/edgion-plugins'
import type { EdgionPlugins, EdgionPluginsSpec } from '@/types/edgion-plugins'

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
        credentials:
          - username: admin
            password: secret
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
 * 规范化 EdgionPlugins（填充缺失的默认值）
 */
export function normalizeEdgionPlugins(resource: EdgionPlugins | Record<string, unknown>): EdgionPlugins {
  const r = resource as any
  return {
    apiVersion: r.apiVersion || EDGION_PLUGINS_API_VERSION,
    kind: EDGION_PLUGINS_KIND,
    metadata: {
      name: r.metadata?.name || '',
      namespace: r.metadata?.namespace || 'default',
      labels: r.metadata?.labels || {},
      annotations: r.metadata?.annotations || {},
      ...(r.metadata?.uid ? { uid: r.metadata.uid } : {}),
      ...(r.metadata?.resourceVersion ? { resourceVersion: r.metadata.resourceVersion } : {}),
      ...(r.metadata?.creationTimestamp ? { creationTimestamp: r.metadata.creationTimestamp } : {}),
    },
    spec: r.spec || {},
    ...(r.status ? { status: r.status } : {}),
  }
}

/**
 * 将 EdgionPlugins 序列化为 YAML 字符串
 * 遵循 Edgion 的 serde 规则：省略空数组、默认 true 的 enable 字段
 */
export function edgionPluginsToYAML(resource: EdgionPlugins): string {
  const clean: Record<string, unknown> = {
    apiVersion: resource.apiVersion || EDGION_PLUGINS_API_VERSION,
    kind: resource.kind || EDGION_PLUGINS_KIND,
    metadata: {},
  }

  // metadata - 只序列化非空字段
  const meta: Record<string, unknown> = {}
  if (resource.metadata?.name) meta.name = resource.metadata.name
  if (resource.metadata?.namespace) meta.namespace = resource.metadata.namespace
  if (resource.metadata?.labels && Object.keys(resource.metadata.labels).length > 0) {
    meta.labels = resource.metadata.labels
  }
  if (resource.metadata?.annotations && Object.keys(resource.metadata.annotations).length > 0) {
    meta.annotations = resource.metadata.annotations
  }
  clean.metadata = meta

  // spec - 省略空数组
  const spec: Record<string, unknown> = {}
  if (resource.spec?.requestPlugins?.length) {
    spec.requestPlugins = resource.spec.requestPlugins
  }
  if (resource.spec?.upstreamResponseFilterPlugins?.length) {
    spec.upstreamResponseFilterPlugins = resource.spec.upstreamResponseFilterPlugins
  }
  if (resource.spec?.upstreamResponseBodyFilterPlugins?.length) {
    spec.upstreamResponseBodyFilterPlugins = resource.spec.upstreamResponseBodyFilterPlugins
  }
  if (resource.spec?.upstreamResponsePlugins?.length) {
    spec.upstreamResponsePlugins = resource.spec.upstreamResponsePlugins
  }
  clean.spec = spec

  return yaml.dump(clean, {
    indent: 2,
    noRefs: true,
    lineWidth: -1,
  })
}

/**
 * 将 YAML 字符串解析为 EdgionPlugins 对象
 */
export function yamlToEdgionPlugins(yamlStr: string): EdgionPlugins {
  const parsed = yaml.load(yamlStr)
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('无效的 YAML：期望对象格式')
  }
  return parsed as EdgionPlugins
}

export function countPluginsByStage(spec: EdgionPluginsSpec | undefined) {
  return {
    request: spec?.requestPlugins?.length ?? 0,
    responseFilter: spec?.upstreamResponseFilterPlugins?.length ?? 0,
    responseBodyFilter: spec?.upstreamResponseBodyFilterPlugins?.length ?? 0,
    response: spec?.upstreamResponsePlugins?.length ?? 0,
  }
}
