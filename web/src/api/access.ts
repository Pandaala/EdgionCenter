import { apiClient } from './client'
import type { AxiosRequestConfig } from 'axios'
import {
  CONTROLLER_ACCESS_OPERATIONS,
  CONTROLLER_ACCESS_RESOURCE_VERBS,
  type ApiResponse,
  type ControllerAccessDocument,
  type ControllerAccessOperation,
  type ControllerAccessResourceVerb,
  type ResourceKind,
  type ResourceScope,
} from './types'

export const CONTROLLER_KIND_BY_RESOURCE_KIND: Readonly<Record<ResourceKind, string>> = {
  httproute: 'HTTPRoute',
  grpcroute: 'GRPCRoute',
  tcproute: 'TCPRoute',
  udproute: 'UDPRoute',
  tlsroute: 'TLSRoute',
  service: 'Service',
  endpointslice: 'EndpointSlice',
  edgiontls: 'EdgionTls',
  edgionplugins: 'EdgionPlugins',
  edgionconfigdata: 'EdgionConfigData',
  linksys: 'LinkSys',
  secret: 'Secret',
  gatewayclass: 'GatewayClass',
  edgiongatewayconfig: 'EdgionGatewayConfig',
  gateway: 'Gateway',
  edgionstreamplugins: 'EdgionStreamPlugins',
  referencegrant: 'ReferenceGrant',
  edgionacme: 'EdgionAcme',
  backendtlspolicy: 'BackendTLSPolicy',
  edgionbackendtrafficpolicy: 'EdgionBackendTrafficPolicy',
  configmap: 'ConfigMap',
}

const SCOPE_BY_RESOURCE_KIND: Readonly<Record<ResourceKind, ResourceScope>> = {
  httproute: 'namespaced',
  grpcroute: 'namespaced',
  tcproute: 'namespaced',
  udproute: 'namespaced',
  tlsroute: 'namespaced',
  service: 'namespaced',
  endpointslice: 'namespaced',
  edgiontls: 'namespaced',
  edgionplugins: 'namespaced',
  edgionconfigdata: 'namespaced',
  linksys: 'namespaced',
  secret: 'namespaced',
  gatewayclass: 'cluster',
  edgiongatewayconfig: 'cluster',
  gateway: 'namespaced',
  edgionstreamplugins: 'namespaced',
  referencegrant: 'namespaced',
  edgionacme: 'namespaced',
  backendtlspolicy: 'namespaced',
  edgionbackendtrafficpolicy: 'namespaced',
  configmap: 'namespaced',
}

/** ResourceKind::as_str() declaration order, excluding Unspecified. */
export const CONTROLLER_ACCESS_RESOURCE_KINDS: readonly ResourceKind[] = [
  'gatewayclass',
  'edgiongatewayconfig',
  'gateway',
  'httproute',
  'service',
  'endpointslice',
  'edgiontls',
  'secret',
  'edgionplugins',
  'grpcroute',
  'tcproute',
  'udproute',
  'edgionconfigdata',
  'tlsroute',
  'linksys',
  'edgionstreamplugins',
  'referencegrant',
  'backendtlspolicy',
  'edgionacme',
  'configmap',
  'edgionbackendtrafficpolicy',
]
const VERB_INDEX = new Map(CONTROLLER_ACCESS_RESOURCE_VERBS.map((verb, index) => [verb, index]))

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isOrderedVerbList(value: unknown): value is ControllerAccessResourceVerb[] {
  if (!Array.isArray(value)) return false
  let lastIndex = -1
  const seen = new Set<string>()
  for (const verb of value) {
    if (typeof verb !== 'string' || seen.has(verb)) return false
    const index = VERB_INDEX.get(verb as ControllerAccessResourceVerb)
    if (index === undefined || index <= lastIndex) return false
    seen.add(verb)
    lastIndex = index
  }
  return true
}

/**
 * Validate the complete bounded v1 contract. Missing/duplicate resource or
 * operation rows are incompatible instead of being interpreted as denial.
 */
export function parseControllerAccessDocument(value: unknown): ControllerAccessDocument {
  if (!isRecord(value) || value.schemaVersion !== 1) {
    throw new Error('Unsupported Controller access schema')
  }
  if (typeof value.revision !== 'string' || !/^sha256:[0-9a-f]{64}$/.test(value.revision)) {
    throw new Error('Invalid Controller access revision')
  }
  if (!Array.isArray(value.resources) || value.resources.length !== CONTROLLER_ACCESS_RESOURCE_KINDS.length) {
    throw new Error('Controller access resource catalog is incomplete')
  }

  const resources = value.resources.map((row, index) => {
    if (!isRecord(row) || typeof row.kind !== 'string') {
      throw new Error('Invalid Controller access resource row')
    }
    const resourceKind = CONTROLLER_ACCESS_RESOURCE_KINDS[index]
    const expectedKind = CONTROLLER_KIND_BY_RESOURCE_KIND[resourceKind]
    const expectedScope = SCOPE_BY_RESOURCE_KIND[resourceKind]
    if (row.kind !== expectedKind || row.scope !== expectedScope || !isOrderedVerbList(row.verbs)) {
      throw new Error(`Invalid Controller access row for ${row.kind}`)
    }
    return { kind: row.kind, scope: expectedScope, verbs: row.verbs }
  })

  if (!Array.isArray(value.operations) || value.operations.length !== CONTROLLER_ACCESS_OPERATIONS.length) {
    throw new Error('Controller access operation catalog is incomplete')
  }
  const operations = value.operations.map((row, index) => {
    const expectedName = CONTROLLER_ACCESS_OPERATIONS[index]
    if (!isRecord(row) || row.name !== expectedName || typeof row.allowed !== 'boolean') {
      throw new Error(`Invalid Controller access operation row for ${expectedName}`)
    }
    return { name: expectedName, allowed: row.allowed }
  })

  return {
    schemaVersion: 1,
    revision: value.revision as `sha256:${string}`,
    resources,
    operations,
  }
}

export function controllerAccessPath(controllerId: string | null): string {
  if (!controllerId) return '/api/v1/access'
  return `/api/v1/proxy/${controllerId.replace(/\//g, '~')}/api/v1/access`
}

export const controllerAccessApi = {
  get: async (controllerId: string | null): Promise<ControllerAccessDocument> => {
    const config: AxiosRequestConfig & { _skipControllerProxy: boolean; _silent: boolean } = {
      baseURL: '/',
      _skipControllerProxy: true,
      _silent: true,
    }
    const { data } = await apiClient.get<ApiResponse<unknown>>(controllerAccessPath(controllerId), config)
    if (!data.success || data.data === undefined) {
      throw new Error(data.error || 'Controller access response is unsuccessful')
    }
    return parseControllerAccessDocument(data.data)
  },
}

export function controllerKindFor(resourceKind: ResourceKind): string {
  return CONTROLLER_KIND_BY_RESOURCE_KIND[resourceKind]
}

export function operationIsAllowed(
  document: ControllerAccessDocument,
  operation: ControllerAccessOperation,
): boolean {
  return document.operations.find((row) => row.name === operation)?.allowed === true
}
