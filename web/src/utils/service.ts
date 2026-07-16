import * as yaml from 'js-yaml'
import { mutationDocumentToYaml, withCreateDefaults } from './resource-document'

export interface ServiceResource {
  apiVersion: 'v1'; kind: 'Service'
  metadata: { name: string; namespace: string; labels?: Record<string, string>; annotations?: Record<string, string> }
  spec: { type?: string; selector?: Record<string, string>; ports?: Array<{ name?: string; protocol?: 'TCP' | 'UDP' | 'SCTP'; port: number; targetPort?: number | string; appProtocol?: string }>; clusterIP?: string; externalName?: string; sessionAffinity?: string; [key: string]: unknown }
  [key: string]: unknown
}
const defaults: ServiceResource = { apiVersion: 'v1', kind: 'Service', metadata: { name: '', namespace: 'default' }, spec: { type: 'ClusterIP', selector: {}, ports: [] } }
export const createEmptyService = () => withCreateDefaults(undefined, defaults)
export const normalizeService = (raw: unknown) => withCreateDefaults(raw, defaults)
export function serviceFromYaml(value: string) { const raw = yaml.load(value) as any; if (raw?.kind !== 'Service') throw new Error('YAML must contain a Service'); return normalizeService(raw) }
export const serviceToYaml = (value: ServiceResource, mode: 'create' | 'update') => mutationDocumentToYaml(value, 'service', mode)
export function validateService(value: ServiceResource) {
  if (!value.metadata.name || !value.metadata.namespace) throw new Error('Name and namespace are required')
  const names = new Set<string>()
  value.spec.ports?.forEach((port) => { if (!Number.isInteger(port.port) || port.port < 1 || port.port > 65535) throw new Error('Service ports must be between 1 and 65535'); if (port.name && names.has(port.name)) throw new Error(`Duplicate port name: ${port.name}`); if (port.name) names.add(port.name) })
  if (value.spec.type === 'ExternalName' && !value.spec.externalName) throw new Error('ExternalName is required for ExternalName services')
}
