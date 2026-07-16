/**
 * EdgionPlugins 工具函数
 */

import * as yaml from 'js-yaml'
import { EDGION_PLUGINS_API_VERSION, EDGION_PLUGINS_KIND } from '@/types/edgion-plugins'
import type { EdgionPlugins, EdgionPluginsSpec } from '@/types/edgion-plugins'
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
  if (!resource || typeof resource !== 'object' || resource.kind !== EDGION_PLUGINS_KIND) {
    throw new Error('Expected an EdgionPlugins document')
  }
  if (!resource.metadata || typeof resource.metadata !== 'object' || !resource.spec || typeof resource.spec !== 'object') {
    throw new Error('EdgionPlugins metadata and spec are required')
  }
  return structuredClone(resource) as EdgionPlugins
}

/**
 * 将 EdgionPlugins 序列化为 YAML 字符串
 * 遵循 Edgion 的 serde 规则：省略空数组、默认 true 的 enable 字段
 */
export function edgionPluginsToYAML(resource: EdgionPlugins): string {
  return dumpYaml(resource)
}

export function edgionPluginsToMutationYAML(resource: EdgionPlugins, mode: 'create' | 'update'): string {
  return mutationDocumentToYaml(resource, 'edgionplugins', mode)
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
