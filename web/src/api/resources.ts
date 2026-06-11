import { apiClient } from './client'
import type { ApiResponse, ListResponse, K8sResource, ResourceKey, ResourceKind } from './types'
import * as yaml from 'js-yaml'

export const resourceApi = {
  /**
   * List all resources of a kind (across all namespaces).
   * Supports K8s-style cursor pagination via `limit` + `continue` query params.
   */
  listAll: async <T extends K8sResource>(
    kind: ResourceKind,
    options: { limit?: number; continue?: string } = {}
  ): Promise<ListResponse<T>> => {
    const params = new URLSearchParams()
    if (options.limit) params.set('limit', String(options.limit))
    if (options.continue) params.set('continue', options.continue)
    const qs = params.toString() ? `?${params.toString()}` : ''
    const { data } = await apiClient.get(`/namespaced/${kind}${qs}`)
    return data
  },

  /**
   * List metadata-only resource keys (no spec/status).
   *
   * Hits `/api/v1/keys/namespaced/{kind}[/{namespace}]`. Supports K8s-style
   * pagination via `limit` + `continue`. Use when only metadata is needed —
   * Topology and pages reading `record.spec.*` stay on `listAll` / `list`.
   */
  listKeys: async (
    kind: ResourceKind,
    options: { namespace?: string; limit?: number; continue?: string } = {}
  ): Promise<ListResponse<ResourceKey>> => {
    const params = new URLSearchParams()
    if (options.limit) params.set('limit', String(options.limit))
    if (options.continue) params.set('continue', options.continue)
    const qs = params.toString() ? `?${params.toString()}` : ''
    const path = options.namespace
      ? `/keys/namespaced/${kind}/${options.namespace}${qs}`
      : `/keys/namespaced/${kind}${qs}`
    const { data } = await apiClient.get(path)
    return data
  },

  /**
   * List resources in a specific namespace
   */
  list: async <T extends K8sResource>(
    kind: ResourceKind,
    namespace: string
  ): Promise<ListResponse<T>> => {
    const { data } = await apiClient.get(`/namespaced/${kind}/${namespace}`)
    return data
  },

  /**
   * Get a single resource
   */
  get: async <T extends K8sResource>(
    kind: ResourceKind,
    namespace: string,
    name: string
  ): Promise<T> => {
    const { data } = await apiClient.get(`/namespaced/${kind}/${namespace}/${name}`)
    return data
  },

  /**
   * Create a resource
   */
  create: async <T extends K8sResource>(
    kind: ResourceKind,
    namespace: string,
    resource: T | string
  ): Promise<ApiResponse<string>> => {
    const content = typeof resource === 'string' ? resource : yaml.dump(resource)
    const { data } = await apiClient.post(`/namespaced/${kind}/${namespace}`, content, {
      headers: { 'Content-Type': 'application/yaml' },
    })
    return data
  },

  /**
   * Update a resource
   */
  update: async <T extends K8sResource>(
    kind: ResourceKind,
    namespace: string,
    name: string,
    resource: T | string
  ): Promise<ApiResponse<string>> => {
    const content = typeof resource === 'string' ? resource : yaml.dump(resource)
    const { data } = await apiClient.put(`/namespaced/${kind}/${namespace}/${name}`, content, {
      headers: { 'Content-Type': 'application/yaml' },
    })
    return data
  },

  /**
   * Delete a resource
   */
  delete: async (
    kind: ResourceKind,
    namespace: string,
    name: string
  ): Promise<ApiResponse<string>> => {
    const { data } = await apiClient.delete(`/namespaced/${kind}/${namespace}/${name}`)
    return data
  },

  /**
   * Batch delete resources
   */
  batchDelete: async (
    kind: ResourceKind,
    resources: Array<{ namespace: string; name: string }>
  ): Promise<void> => {
    await Promise.all(
      resources.map((r) => resourceApi.delete(kind, r.namespace, r.name))
    )
  },
}

// Cluster-scoped resources API
export const clusterResourceApi = {
  listAll: async <T extends K8sResource>(
    kind: ResourceKind,
    options: { limit?: number; continue?: string } = {}
  ): Promise<ListResponse<T>> => {
    const params = new URLSearchParams()
    if (options.limit) params.set('limit', String(options.limit))
    if (options.continue) params.set('continue', options.continue)
    const qs = params.toString() ? `?${params.toString()}` : ''
    const { data } = await apiClient.get(`/cluster/${kind}${qs}`)
    return data
  },

  /**
   * List metadata-only resource keys for a cluster-scoped kind.
   *
   * Hits `/api/v1/keys/cluster/{kind}` with K8s-style `limit` + `continue`.
   */
  listKeys: async (
    kind: ResourceKind,
    options: { limit?: number; continue?: string } = {}
  ): Promise<ListResponse<ResourceKey>> => {
    const params = new URLSearchParams()
    if (options.limit) params.set('limit', String(options.limit))
    if (options.continue) params.set('continue', options.continue)
    const qs = params.toString() ? `?${params.toString()}` : ''
    const { data } = await apiClient.get(`/keys/cluster/${kind}${qs}`)
    return data
  },

  get: async <T extends K8sResource>(kind: ResourceKind, name: string): Promise<T> => {
    const { data } = await apiClient.get(`/cluster/${kind}/${name}`)
    return data
  },

  create: async <T extends K8sResource>(
    kind: ResourceKind,
    resource: T | string
  ): Promise<ApiResponse<string>> => {
    const content = typeof resource === 'string' ? resource : yaml.dump(resource)
    const { data } = await apiClient.post(`/cluster/${kind}`, content, {
      headers: { 'Content-Type': 'application/yaml' },
    })
    return data
  },

  update: async <T extends K8sResource>(
    kind: ResourceKind,
    name: string,
    resource: T | string
  ): Promise<ApiResponse<string>> => {
    const content = typeof resource === 'string' ? resource : yaml.dump(resource)
    const { data } = await apiClient.put(`/cluster/${kind}/${name}`, content, {
      headers: { 'Content-Type': 'application/yaml' },
    })
    return data
  },

  delete: async (kind: ResourceKind, name: string): Promise<ApiResponse<string>> => {
    const { data } = await apiClient.delete(`/cluster/${kind}/${name}`)
    return data
  },
}

