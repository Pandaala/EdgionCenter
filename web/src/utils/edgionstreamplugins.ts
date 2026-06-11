/**
 * EdgionStreamPlugins 工具函数
 */

import * as yaml from 'js-yaml'
import type { EdgionStreamPlugins } from '@/types/edgion-stream-plugins'
import { dumpYaml } from './yaml-utils'

export const DEFAULT_YAML = `apiVersion: edgion.io/v1
kind: EdgionStreamPlugins
metadata:
  name: my-stream-plugins
  namespace: default
spec:
  plugins:
    - type: IpRestriction
      config:
        ipSource: remoteAddr
        allow:
          - "10.0.0.0/8"
        defaultAction: allow
`

export function createEmpty(): EdgionStreamPlugins {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'EdgionStreamPlugins',
    metadata: { name: '', namespace: 'default' },
    spec: {
      plugins: [{
        type: 'IpRestriction',
        config: { ipSource: 'remoteAddr', allow: [], deny: [], defaultAction: 'allow', message: '' },
      }],
    },
  }
}

export function normalize(raw: any): EdgionStreamPlugins {
  return {
    apiVersion: raw.apiVersion || 'edgion.io/v1',
    kind: 'EdgionStreamPlugins',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: { plugins: raw.spec?.plugins || [] },
    status: raw.status,
  }
}

export function toYaml(sp: EdgionStreamPlugins): string {
  return dumpYaml(sp)
}

export function fromYaml(yamlStr: string): EdgionStreamPlugins {
  return normalize(yaml.load(yamlStr) as any)
}
