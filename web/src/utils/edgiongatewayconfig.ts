import * as yaml from 'js-yaml'
import type { EdgionGatewayConfig } from '@/types/edgion-gateway-config'
import { dumpYaml } from './yaml-utils'

export const DEFAULT_YAML = `apiVersion: edgion.io/v1alpha1
kind: EdgionGatewayConfig
metadata:
  name: default-config
spec:
  server:
    gracePeriodSeconds: 30
    gracefulShutdownTimeoutS: 10
  httpTimeout:
    client:
      readTimeout: "60s"
      writeTimeout: "60s"
    backend:
      defaultConnectTimeout: "5s"
      defaultRequestTimeout: "60s"
  maxRetries: 3
  preflightPolicy:
    mode: cors-standard
    statusCode: 204
`

export function createEmpty(): EdgionGatewayConfig {
  return {
    apiVersion: 'edgion.io/v1alpha1',
    kind: 'EdgionGatewayConfig',
    metadata: { name: 'default-config' },
    spec: {
      server: { gracePeriodSeconds: 30 },
      httpTimeout: {
        client: { readTimeout: '60s', writeTimeout: '60s' },
        backend: { defaultConnectTimeout: '5s', defaultRequestTimeout: '60s' },
      },
      maxRetries: 3,
      preflightPolicy: { mode: 'cors-standard', statusCode: 204 },
    },
  }
}

export function normalize(raw: any): EdgionGatewayConfig {
  return {
    apiVersion: raw.apiVersion || 'edgion.io/v1alpha1',
    kind: 'EdgionGatewayConfig',
    metadata: {
      name: raw.metadata?.name || '',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: raw.spec || {},
    status: raw.status,
  }
}

export function toYaml(cfg: EdgionGatewayConfig): string {
  return dumpYaml(cfg)
}

export function fromYaml(yamlStr: string): EdgionGatewayConfig {
  return normalize(yaml.load(yamlStr) as any)
}
