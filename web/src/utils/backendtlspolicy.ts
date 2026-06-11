import * as yaml from 'js-yaml'
import { dumpYaml } from './yaml-utils'

export interface BackendTLSPolicyTargetRef {
  group: string
  kind: string
  name: string
}

export interface BackendTLSPolicyCACertRef {
  name: string
  group: string
  kind: string
}

export interface BackendTLSPolicy {
  apiVersion: string
  kind: string
  metadata: {
    name: string
    namespace?: string
    labels?: Record<string, string>
    annotations?: Record<string, string>
    resourceVersion?: string
    creationTimestamp?: string
  }
  spec: {
    targetRefs: BackendTLSPolicyTargetRef[]
    validation: {
      hostname: string
      caCertificateRefs: BackendTLSPolicyCACertRef[]
    }
  }
  status?: any
}

export function createEmpty(): BackendTLSPolicy {
  return {
    apiVersion: 'gateway.networking.k8s.io/v1alpha3',
    kind: 'BackendTLSPolicy',
    metadata: { name: '', namespace: 'default' },
    spec: {
      targetRefs: [{ group: '', kind: 'Service', name: '' }],
      validation: {
        hostname: '',
        caCertificateRefs: [{ name: '', group: '', kind: 'Secret' }],
      },
    },
  }
}

export function normalize(raw: any): BackendTLSPolicy {
  const targetRefs: BackendTLSPolicyTargetRef[] = (raw.spec?.targetRefs || []).map((r: any) => ({
    group: r.group ?? '',
    kind: r.kind || 'Service',
    name: r.name || '',
  }))
  const caCertificateRefs: BackendTLSPolicyCACertRef[] = (raw.spec?.validation?.caCertificateRefs || []).map((r: any) => ({
    name: r.name || '',
    group: r.group ?? '',
    kind: r.kind || 'Secret',
  }))
  return {
    apiVersion: raw.apiVersion || 'gateway.networking.k8s.io/v1alpha3',
    kind: 'BackendTLSPolicy',
    metadata: {
      name: raw.metadata?.name || '',
      namespace: raw.metadata?.namespace || 'default',
      labels: raw.metadata?.labels,
      annotations: raw.metadata?.annotations,
      resourceVersion: raw.metadata?.resourceVersion,
      creationTimestamp: raw.metadata?.creationTimestamp,
    },
    spec: {
      targetRefs: targetRefs.length > 0 ? targetRefs : [{ group: '', kind: 'Service', name: '' }],
      validation: {
        hostname: raw.spec?.validation?.hostname || '',
        caCertificateRefs: caCertificateRefs.length > 0 ? caCertificateRefs : [{ name: '', group: '', kind: 'Secret' }],
      },
    },
    status: raw.status,
  }
}

export function toYaml(policy: BackendTLSPolicy): string {
  return dumpYaml(policy)
}

export function fromYaml(yamlStr: string): BackendTLSPolicy {
  return normalize(yaml.load(yamlStr) as any)
}
