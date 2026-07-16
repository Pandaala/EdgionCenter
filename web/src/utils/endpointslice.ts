import * as yaml from 'js-yaml'
import { mutationDocumentToYaml, withCreateDefaults } from './resource-document'

export interface EndpointSliceResource {
  apiVersion: 'discovery.k8s.io/v1'; kind: 'EndpointSlice'
  metadata: { name: string; namespace: string; labels?: Record<string, string>; annotations?: Record<string, string> }
  addressType: 'IPv4' | 'IPv6' | 'FQDN'
  ports?: Array<{ name?: string; protocol?: 'TCP' | 'UDP' | 'SCTP'; port?: number; appProtocol?: string }>
  endpoints: Array<{ addresses: string[]; conditions?: { ready?: boolean; serving?: boolean; terminating?: boolean }; hostname?: string; nodeName?: string; zone?: string; hints?: unknown; targetRef?: unknown }>
  [key: string]: unknown
}
const defaults: EndpointSliceResource = { apiVersion: 'discovery.k8s.io/v1', kind: 'EndpointSlice', metadata: { name: '', namespace: 'default', labels: { 'kubernetes.io/service-name': '' } }, addressType: 'IPv4', ports: [], endpoints: [] }
export const createEmptyEndpointSlice = () => withCreateDefaults(undefined, defaults)
export const normalizeEndpointSlice = (raw: unknown) => withCreateDefaults(raw, defaults)
export function endpointSliceFromYaml(value: string) { const raw = yaml.load(value) as any; if (raw?.kind !== 'EndpointSlice') throw new Error('YAML must contain an EndpointSlice'); return normalizeEndpointSlice(raw) }
export const endpointSliceToYaml = (value: EndpointSliceResource, mode: 'create' | 'update') => mutationDocumentToYaml(value, 'endpointslice', mode)
export function validateEndpointSlice(value: EndpointSliceResource) {
  if (!value.metadata.name || !value.metadata.namespace) throw new Error('Name and namespace are required')
  if (!value.metadata.labels?.['kubernetes.io/service-name']) throw new Error('kubernetes.io/service-name label is required')
  value.ports?.forEach((port) => { if (port.port !== undefined && (!Number.isInteger(port.port) || port.port < 1 || port.port > 65535)) throw new Error('EndpointSlice ports must be between 1 and 65535') })
  value.endpoints.forEach((endpoint) => { if (!endpoint.addresses.length) throw new Error('Every endpoint needs at least one address') })
}
