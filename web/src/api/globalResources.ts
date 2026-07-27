import type { AxiosRequestConfig } from 'axios'
import { apiClient } from './client'

const GLOBAL_RESOURCES_BASE = '/api/v1/center/global-resources'

type CenterRequestConfig = AxiosRequestConfig & {
  _skipControllerProxy: true
}

const centerRequest: CenterRequestConfig = {
  _skipControllerProxy: true,
}

export type GlobalResourceKind = 'EdgionConfigData'

export type GlobalResourceApiSlug = 'edgion-config-data'

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
  kinds: GlobalResourceCatalogKind[]
  clusters: GlobalResourceClusterResolution[]
}

export interface GlobalResourceClusterError {
  namespace: string
  code: InventoryErrorCode
  status: number | null
  retryable: boolean
}

export type WatchSyncState = 'ok' | 'stale' | 'overflowed'

export interface GlobalResourceClusterResult extends GlobalResourceClusterResolution {
  complete: boolean
  errors: GlobalResourceClusterError[]
  syncState?: WatchSyncState
  freshnessUnixMs?: number
  revision?: number
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
  syncState?: WatchSyncState
  freshnessUnixMs?: number
  revision?: number
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

export interface GlobalResourceListOptions {
  clusters?: readonly string[]
  configDataType?: EdgionConfigDataFilter
  limit?: number
  continueToken?: string
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
}
