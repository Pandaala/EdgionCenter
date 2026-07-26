import type { AxiosRequestConfig } from 'axios'
import { apiClient } from './client'

const GLOBAL_RESOURCES_BASE = '/api/v1/center/global-resources'
const GLOBAL_RESOURCE_SYNC_BASE = '/api/v1/center/global-resource-sync/resources'

type CenterRequestConfig = AxiosRequestConfig & {
  _skipControllerProxy: true
}

const centerRequest: CenterRequestConfig = {
  _skipControllerProxy: true,
}

export type GlobalResourceKind =
  | 'HTTPRoute'
  | 'GRPCRoute'
  | 'EdgionPlugins'
  | 'EdgionConfigData'
  | 'ReferenceGrant'

export type GlobalResourceApiSlug =
  | 'http-route'
  | 'grpc-route'
  | 'edgion-plugins'
  | 'edgion-config-data'
  | 'reference-grant'

export type EdgionConfigDataType =
  | 'KeyList'
  | 'IpList'
  | 'Selector'
  | 'RegionRouteOverride'
  | 'Misc'

export type EdgionConfigDataFilter = EdgionConfigDataType | 'Unknown'

export type ClusterResolutionState =
  | 'available'
  | 'offline'
  | 'ambiguous'
  | 'indeterminate'

export type InventoryErrorCode =
  | 'cluster_not_found'
  | 'controller_offline'
  | 'controller_ambiguous'
  | 'ownership_indeterminate'
  | 'upstream_unavailable'
  | 'upstream_rejected'
  | 'payload_too_large'
  | 'invalid_upstream_response'
  | 'budget_exceeded'
  | 'pagination_invalid'
  | 'pagination_limit'

export type NamespacePreflightState =
  | 'available'
  | 'access_denied'
  | 'backend_rejected'
  | 'unavailable'

export type JsonPrimitive = boolean | number | string | null
export type JsonValue = JsonPrimitive | JsonObject | JsonValue[]
export interface JsonObject {
  [key: string]: JsonValue
}

export interface GlobalResourceClusterResolution {
  cluster: string
  state: ClusterResolutionState
  controllerId: string | null
  candidates: string[]
}

export interface GlobalResourceCatalogKind {
  kind: GlobalResourceKind
  slug: GlobalResourceApiSlug
  configDataTypes: EdgionConfigDataType[]
}

export interface GlobalResourceCatalogResponse {
  catalogRevision: string
  configRevision: string
  platformNamespaces: string[]
  kinds: GlobalResourceCatalogKind[]
  clusters: GlobalResourceClusterResolution[]
}

export interface GlobalResourceClusterError {
  namespace: string
  code: InventoryErrorCode
  status: number | null
  retryable: boolean
}

export interface GlobalResourceClusterResult extends GlobalResourceClusterResolution {
  complete: boolean
  errors: GlobalResourceClusterError[]
}

export interface GlobalResourceGroupKey {
  kind: string
  namespace: string
  name: string
}

export interface GlobalResourceComparisonMember {
  cluster: string
  controllerId: string | null
  object: JsonObject
}

export interface GlobalResourceComparisonGroup {
  key: GlobalResourceGroupKey
  members: GlobalResourceComparisonMember[]
}

export interface GlobalResourceListResponse {
  catalogRevision: string
  configRevision: string
  membershipRevision: string
  inventoryRevision: string
  kind: GlobalResourceKind
  configDataType: EdgionConfigDataFilter | null
  clusters: GlobalResourceClusterResult[]
  groups: GlobalResourceComparisonGroup[]
  continueToken: string | null
}

export interface GlobalResourceDetailResponse extends GlobalResourceClusterResult {
  kind: GlobalResourceKind
  namespace: string
  name: string
  object: JsonObject | null
}

export interface GlobalResourceKindPreflight {
  kind: string
  canGet: boolean
  canList: boolean
  canCreate: boolean
  canUpdate: boolean
  mutationAvailable: boolean
}

export interface GlobalResourceNamespacePreflight {
  namespace: string
  kind: string
  state: NamespacePreflightState
  detail: string | null
}

export interface ControllerGlobalResourcesPreflight {
  controllerId: string
  accessRevision: string
  catalogRevision: string
  inventoryAvailable: boolean
  syncAvailable: boolean
  kinds: GlobalResourceKindPreflight[]
  namespaces: GlobalResourceNamespacePreflight[]
}

export interface GlobalResourcePreflightResponse {
  cluster: string
  preflight: ControllerGlobalResourcesPreflight
}

export interface GlobalResourceListOptions {
  clusters?: readonly string[]
  configDataType?: EdgionConfigDataFilter
  limit?: number
  continueToken?: string
}

export interface GlobalResourceDesired {
  displayName: string
  resourceKind: GlobalResourceKind
  targetNamespace: string
  templateDocument: JsonObject
  targetSelector: { type: string; clusters?: string[]; metadata?: Record<string, string> }
  syncPolicy: { mode: string; adoption: string; prune: string }
}

export interface DurableGlobalResource {
  id: string
  desired: GlobalResourceDesired
  generation: number
  desiredRevision: string
  createdBy?: string
  updatedBy?: string
  updatedAtUnixMs?: number
}

export interface DurableGlobalResourcePage { data: DurableGlobalResource[]; continueToken?: string | null }
export interface GlobalResourcePlanTarget {
  cluster: string
  state: string
  reason?: string
  controllerId?: string | null
  observedResourceVersion?: string | null
  changedPaths?: string[]
}
export interface GlobalResourcePlan {
  globalResourceId: string
  generation: number
  desiredRevision: string
  targetClusters: string[]
  planToken: string
  applicable: boolean
  targets: GlobalResourcePlanTarget[]
}
export interface GlobalResourceApplyTarget extends GlobalResourcePlanTarget {
  statusCode?: number | null
  requestId?: string
}
export interface GlobalResourceApplyResult { targets: GlobalResourceApplyTarget[]; [key: string]: unknown }

function syncPath(id?: string): string {
  return id ? `${GLOBAL_RESOURCE_SYNC_BASE}/${encodeURIComponent(id)}` : GLOBAL_RESOURCE_SYNC_BASE
}

function unwrap<T>(value: T | { data: T }): T {
  return (value && typeof value === 'object' && 'data' in value) ? (value as { data: T }).data : value as T
}

function resourcePath(kind: GlobalResourceApiSlug): string {
  return `${GLOBAL_RESOURCES_BASE}/resources/${kind}`
}

function appendListParams(
  params: URLSearchParams,
  options: GlobalResourceListOptions,
): URLSearchParams {
  options.clusters?.forEach((cluster) => params.append('cluster', cluster))
  if (options.configDataType !== undefined) {
    params.set('configDataType', options.configDataType)
  }
  if (options.limit !== undefined) {
    params.set('limit', String(options.limit))
  }
  if (options.continueToken !== undefined) {
    params.set('continue', options.continueToken)
  }
  return params
}

export const globalResourcesApi = {
  catalog: async (): Promise<GlobalResourceCatalogResponse> => {
    const { data } = await apiClient.get<GlobalResourceCatalogResponse>(
      `${GLOBAL_RESOURCES_BASE}/catalog`,
      centerRequest,
    )
    return data
  },

  list: async (
    kind: GlobalResourceApiSlug,
    options: GlobalResourceListOptions = {},
  ): Promise<GlobalResourceListResponse> => {
    const params = appendListParams(new URLSearchParams(), options)
    const { data } = await apiClient.get<GlobalResourceListResponse>(
      resourcePath(kind),
      { ...centerRequest, params },
    )
    return data
  },

  detail: async (
    kind: GlobalResourceApiSlug,
    namespace: string,
    name: string,
    cluster: string,
  ): Promise<GlobalResourceDetailResponse> => {
    const params = new URLSearchParams()
    params.set('cluster', cluster)
    const { data } = await apiClient.get<GlobalResourceDetailResponse>(
      `${resourcePath(kind)}/${encodeURIComponent(namespace)}/${encodeURIComponent(name)}`,
      { ...centerRequest, params },
    )
    return data
  },

  preflight: async (cluster: string): Promise<GlobalResourcePreflightResponse> => {
    const params = new URLSearchParams()
    params.set('cluster', cluster)
    const { data } = await apiClient.get<GlobalResourcePreflightResponse>(
      `${GLOBAL_RESOURCES_BASE}/preflight`,
      { ...centerRequest, params },
    )
    return data
  },

  syncList: async (options: { limit?: number; cursor?: string } = {}): Promise<DurableGlobalResourcePage> => {
    const { data } = await apiClient.get(syncPath(), { ...centerRequest, params: options })
    return unwrap(data)
  },

  syncGet: async (id: string): Promise<DurableGlobalResource> => {
    const { data } = await apiClient.get(syncPath(id), centerRequest)
    return unwrap(data)
  },

  syncCreate: async (resource: { id: string; desired: GlobalResourceDesired }): Promise<DurableGlobalResource> => {
    const { data } = await apiClient.post(syncPath(), resource, centerRequest)
    return unwrap(data)
  },

  syncReplace: async (id: string, desired: GlobalResourceDesired, generation: number): Promise<DurableGlobalResource> => {
    const { data } = await apiClient.put(syncPath(id), { desired }, {
      ...centerRequest,
      headers: { ...centerRequest.headers, 'If-Match': `"${generation}"` },
    })
    return unwrap(data)
  },

  syncPlan: async (id: string, targetClusters?: string[]): Promise<GlobalResourcePlan> => {
    const { data } = await apiClient.post(`${syncPath(id)}/plan`, targetClusters ? { targetClusters } : {}, centerRequest)
    return unwrap(data)
  },

  syncApply: async (id: string, input: { planToken: string; generation: number; targetClusters: string[] }): Promise<GlobalResourceApplyResult> => {
    const { data } = await apiClient.post(`${syncPath(id)}/apply`, input, centerRequest)
    return unwrap(data)
  },
}
