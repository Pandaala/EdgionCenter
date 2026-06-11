/**
 * Gateway 工具函数
 */

import * as yaml from 'js-yaml'
import type { Gateway } from '@/types/gateway-api/gateway'
import { removeEmpty } from './yaml-utils'

export const DEFAULT_GATEWAY_YAML = `apiVersion: gateway.networking.k8s.io/v1
kind: Gateway
metadata:
  name: my-gateway
  namespace: default
spec:
  gatewayClassName: edgion
  listeners:
    - name: http
      port: 80
      protocol: HTTP
    - name: https
      port: 443
      protocol: HTTPS
      tls:
        mode: Terminate
        certificateRefs:
          - name: my-cert
`

export function createEmptyGateway(): Gateway {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1',
    kind: 'Gateway',
    metadata: { name: '', namespace: 'default' },
    spec: {
      gatewayClassName: 'edgion',
      listeners: [
        { name: 'http', port: 80, protocol: 'HTTP' },
      ],
    },
  }
}

export function normalizeGateway(raw: any): Gateway {
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1',
    kind: 'Gateway',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: {
      gatewayClassName: raw.spec?.gatewayClassName || 'edgion',
      listeners: raw.spec?.listeners || [],
      addresses: raw.spec?.addresses,
    },
    status: raw.status,
  }
}


export function gatewayToYaml(gw: Gateway): string {
  const clean = removeEmpty(gw)
  return yaml.dump(clean, { lineWidth: -1, noRefs: true })
}

export function yamlToGateway(yamlStr: string): Gateway {
  const raw = yaml.load(yamlStr) as any
  return normalizeGateway(raw)
}
