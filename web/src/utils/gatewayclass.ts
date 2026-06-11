import * as yaml from 'js-yaml'
import { dumpYaml } from './yaml-utils'

export interface GatewayClass {
  apiVersion: string
  kind: string
  metadata: {
    name: string
    resourceVersion?: string
    creationTimestamp?: string
  }
  spec: {
    controllerName: string
    description?: string
  }
  status?: any
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

export function normalize(raw: any): GatewayClass {
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1',
    kind: 'GatewayClass',
    metadata: {
      name: raw.metadata?.name || '',
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: {
      controllerName: raw.spec?.controllerName || '',
      description: raw.spec?.description,
    },
    status: raw.status,
  }
}

export function toYaml(gc: GatewayClass): string {
  return dumpYaml(gc)
}

export function fromYaml(yamlStr: string): GatewayClass {
  return normalize(yaml.load(yamlStr) as any)
}
