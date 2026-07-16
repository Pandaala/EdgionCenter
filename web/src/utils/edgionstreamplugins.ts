import * as yaml from 'js-yaml'
import type { EdgionStreamPlugins } from '@/types/edgion-stream-plugins'
import { dumpYaml } from './yaml-utils'
import { buildMutationDocument } from './resource-document'

export const DEFAULT_YAML = `apiVersion: edgion.io/v1
kind: EdgionStreamPlugins
metadata:
  name: my-stream-plugins
  namespace: default
spec:
  plugins:
    - enable: true
      type: IpRestriction
      config:
        allow:
          - name: private-networks
            cidrs:
              - "10.0.0.0/8"
        defaultAction: deny
`

export function createEmpty(): EdgionStreamPlugins {
  return {
    apiVersion: 'edgion.io/v1',
    kind: 'EdgionStreamPlugins',
    metadata: { name: '', namespace: 'default' },
    spec: {
      plugins: [{
        enable: true,
        type: 'IpRestriction',
        config: { allow: [{ name: 'private-networks', cidrs: [] }], defaultAction: 'deny' },
      }],
      tlsRoutePlugins: [],
    },
  }
}

function clone<T>(value: T): T {
  return structuredClone(value)
}

/** Parse an API view without injecting defaults or projecting known fields. */
export function normalize(raw: unknown): EdgionStreamPlugins {
  if (!raw || typeof raw !== 'object') throw new Error('EdgionStreamPlugins must be an object')
  const resource = raw as EdgionStreamPlugins
  if (resource.kind !== 'EdgionStreamPlugins') throw new Error('Expected EdgionStreamPlugins kind')
  if (!resource.metadata || !resource.spec) throw new Error('EdgionStreamPlugins metadata and spec are required')
  return clone(resource)
}

export function toYaml(sp: EdgionStreamPlugins): string {
  return dumpYaml(sp)
}

export function fromYaml(yamlStr: string): EdgionStreamPlugins {
  return normalize(yaml.load(yamlStr))
}

export function toMutationDocument(
  resource: EdgionStreamPlugins,
  mode: 'create' | 'update',
): Record<string, unknown> {
  return buildMutationDocument(resource, { resourceKind: 'edgionstreamplugins', mode })
}
