import * as yaml from 'js-yaml'
import { dumpYaml } from './yaml-utils'
import { buildMutationDocument } from './resource-document'

export interface GatewayClass {
  apiVersion: string
  kind: string
  metadata: {
    name: string
    labels?: Record<string, string>
    annotations?: Record<string, string>
    resourceVersion?: string
    creationTimestamp?: string
    [key: string]: unknown
  }
  spec: {
    controllerName: string
    description?: string
    parametersRef?: {
      group: string
      kind: string
      name: string
      namespace?: string
      [key: string]: unknown
    }
    [key: string]: unknown
  }
  status?: any
  [key: string]: unknown
}

export function createEmpty(): GatewayClass {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'GatewayClass',
    metadata: { name: '' },
    spec: {
      controllerName: 'edgion.io/gateway-controller',
    },
  }
}

export function normalize(raw: unknown): GatewayClass {
  if (!raw || typeof raw !== 'object') throw new Error('GatewayClass must be an object')
  const resource = raw as GatewayClass
  if (resource.kind !== 'GatewayClass') throw new Error('Expected GatewayClass kind')
  if (!resource.metadata || !resource.spec) throw new Error('GatewayClass metadata and spec are required')
  return structuredClone(resource)
}

export function toYaml(gc: GatewayClass): string {
  return dumpYaml(gc)
}

export function fromYaml(yamlStr: string): GatewayClass {
  return normalize(yaml.load(yamlStr))
}

export function toMutationDocument(
  resource: GatewayClass,
  mode: 'create' | 'update',
): Record<string, unknown> {
  return buildMutationDocument(resource, { resourceKind: 'gatewayclass', mode })
}

export function validateGatewayClass(resource: GatewayClass): string[] {
  const errors: string[] = []
  if (!resource.spec?.controllerName?.trim()) errors.push('spec.controllerName is required')
  const ref = resource.spec?.parametersRef
  if (ref) {
    if (ref.group !== 'edgion.io') errors.push('spec.parametersRef.group must be edgion.io')
    if (ref.kind !== 'EdgionGatewayConfig') errors.push('spec.parametersRef.kind must be EdgionGatewayConfig')
    if (!ref.name?.trim()) errors.push('spec.parametersRef.name is required')
    if (ref.namespace !== undefined) errors.push('spec.parametersRef.namespace is not allowed for cluster-scoped EdgionGatewayConfig')
  }
  return errors
}
