import { apiClient } from './client'
import type { ApiResponse, ListResponse, K8sResource, ResourceKey, ResourceKind } from './types'
import * as yaml from 'js-yaml'
import { controllerAccessApi, controllerKindFor } from './access'
import type { AxiosRequestConfig } from 'axios'

export interface ControllerMutationTarget {
  readonly controllerId: string | null
}

export interface BatchDeleteItem {
  namespace: string
  name: string
  resourceVersion: string
}

export interface BatchDeleteFailure extends BatchDeleteItem {
  error: unknown
}

export interface BatchDeleteResult {
  succeeded: BatchDeleteItem[]
  failed: BatchDeleteFailure[]
}

export class BatchDeletePartialError extends Error {
  constructor(public readonly result: BatchDeleteResult) {
    super(`Deleted ${result.succeeded.length} resources; ${result.failed.length} failed`)
    this.name = 'BatchDeletePartialError'
  }
}

export function batchDeleteFailureKeys(error: unknown): string[] | null {
  if (!(error instanceof BatchDeletePartialError)) return null
  return error.result.failed.map(({ namespace, name }) => `${namespace}/${name}`)
}

function mutationRequestConfig(target: ControllerMutationTarget): AxiosRequestConfig {
  const baseURL = target.controllerId
    ? `/api/v1/proxy/${target.controllerId.replace(/\//g, '~')}/api/v1`
    : '/api/v1'
  return { baseURL, _skipControllerProxy: true } as AxiosRequestConfig
}

function processedRequestConfig(target: ControllerMutationTarget): AxiosRequestConfig {
  const baseURL = target.controllerId
    ? `/api/v1/proxy/${target.controllerId.replace(/\//g, '~')}`
    : '/'
  return { baseURL, _skipControllerProxy: true, _silent: true } as AxiosRequestConfig
}

function resourceVersionHeader(resource: K8sResource | string): Record<string, string> {
  try {
    const document = typeof resource === 'string' ? yaml.load(resource) : resource
    const version = (document as K8sResource | undefined)?.metadata?.resourceVersion
    if (typeof version !== 'string' || version.length === 0) return {}
    // If-Match uses the HTTP entity-tag grammar. The resourceVersion remains in
    // the YAML body as the Kubernetes-compatible concurrency token as well.
    const entityTag = version.replace(/\\/g, '\\\\').replace(/"/g, '\\"')
    return { 'If-Match': `"${entityTag}"` }
  } catch {
    // YAML validation belongs to the Controller. Do not turn an otherwise
    // valid update into a frontend-only parse failure.
    return {}
  }
}

function requiredResourceVersionHeader(version: string): Record<string, string> {
  if (!version) throw new Error('A current resourceVersion is required for deletion')
  return resourceVersionHeader({ apiVersion: '', kind: '', metadata: { name: '', resourceVersion: version } })
}

async function requireResourceMutation(
  target: ControllerMutationTarget,
  kind: ResourceKind,
  verb: 'create' | 'update' | 'delete',
): Promise<AxiosRequestConfig> {
  // Re-read the effective policy immediately before every mutation. Cached UI
  // state controls presentation only; it is never authority for a write.
  const access = await controllerAccessApi.get(target.controllerId)
  const controllerKind = controllerKindFor(kind)
  const allowed = access.resources
    .find((row) => row.kind === controllerKind)
    ?.verbs.includes(verb) === true
  if (!allowed) throw new Error(`Controller denies ${verb} on ${controllerKind}`)
  return mutationRequestConfig(target)
}

export const resourceApi = {
  /**
   * Read the Controller's processed ConfigSync view, including native status.
   * File-system CRUD endpoints intentionally return only the operator-owned
   * resource document; computed status lives in the processed cache/sidecar.
   */
  getProcessed: async <T extends K8sResource>(
    target: ControllerMutationTarget,
    kind: ResourceKind,
    namespace: string | undefined,
    name: string,
  ): Promise<T> => {
    const params = new URLSearchParams({ name })
    if (namespace) params.set('namespace', namespace)
    const { data } = await apiClient.get<ApiResponse<T>>(
      `/configserver/${encodeURIComponent(kind)}?${params}`,
      processedRequestConfig(target),
    )
    if (!data.success || !data.data) throw new Error(data.error || 'Processed resource response is unsuccessful')
    return data.data
  },

  /**
   * List all resources of a kind (across all namespaces).
   * Supports K8s-style cursor pagination via `limit` + `continue` query params.
   */
  listAll: async <T extends K8sResource>(
    kind: ResourceKind,
    options: { limit?: number; continue?: string; silent?: boolean } = {}
  ): Promise<ListResponse<T>> => {
    const params = new URLSearchParams()
    if (options.limit) params.set('limit', String(options.limit))
    if (options.continue) params.set('continue', options.continue)
    const qs = params.toString() ? `?${params.toString()}` : ''
    const { data } = await apiClient.get(`/namespaced/${kind}${qs}`, { _silent: options.silent } as any)
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
    options: { namespace?: string; limit?: number; continue?: string; silent?: boolean } = {}
  ): Promise<ListResponse<ResourceKey>> => {
    const params = new URLSearchParams()
    if (options.limit) params.set('limit', String(options.limit))
    if (options.continue) params.set('continue', options.continue)
    const qs = params.toString() ? `?${params.toString()}` : ''
    const path = options.namespace
      ? `/keys/namespaced/${kind}/${options.namespace}${qs}`
      : `/keys/namespaced/${kind}${qs}`
    const { data } = await apiClient.get(path, { _silent: options.silent } as any)
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
    target: ControllerMutationTarget,
    kind: ResourceKind,
    namespace: string,
    resource: T | string
  ): Promise<ApiResponse<string>> => {
    const content = typeof resource === 'string' ? resource : yaml.dump(resource)
    const config = await requireResourceMutation(target, kind, 'create')
    const { data } = await apiClient.post(`/namespaced/${kind}/${namespace}`, content, {
      ...config,
      headers: { 'Content-Type': 'application/yaml' },
    })
    return data
  },

  /**
   * Update a resource
   */
  update: async <T extends K8sResource>(
    target: ControllerMutationTarget,
    kind: ResourceKind,
    namespace: string,
    name: string,
    resource: T | string
  ): Promise<ApiResponse<string>> => {
    const content = typeof resource === 'string' ? resource : yaml.dump(resource)
    const config = await requireResourceMutation(target, kind, 'update')
    const { data } = await apiClient.put(`/namespaced/${kind}/${namespace}/${name}`, content, {
      ...config,
      headers: { 'Content-Type': 'application/yaml', ...resourceVersionHeader(resource) },
    })
    return data
  },

  /**
   * Delete a resource
   */
  delete: async (
    target: ControllerMutationTarget,
    kind: ResourceKind,
    namespace: string,
    name: string,
    resourceVersion: string,
  ): Promise<ApiResponse<string>> => {
    const config = await requireResourceMutation(target, kind, 'delete')
    const { data } = await apiClient.delete(`/namespaced/${kind}/${namespace}/${name}`, {
      ...config,
      headers: requiredResourceVersionHeader(resourceVersion),
    })
    return data
  },

  /**
   * Batch delete resources
   */
  batchDelete: async (
    target: ControllerMutationTarget,
    kind: ResourceKind,
    resources: BatchDeleteItem[],
    concurrency = 4,
  ): Promise<BatchDeleteResult> => {
    const result: BatchDeleteResult = { succeeded: [], failed: [] }
    const queue = [...resources]
    const worker = async () => {
      for (;;) {
        const resource = queue.shift()
        if (!resource) return
        try {
          await resourceApi.delete(target, kind, resource.namespace, resource.name, resource.resourceVersion)
          result.succeeded.push(resource)
        } catch (error) {
          result.failed.push({ ...resource, error })
        }
      }
    }
    const workerCount = Math.max(1, Math.min(Math.floor(concurrency), resources.length || 1))
    await Promise.all(Array.from({ length: workerCount }, worker))
    if (result.failed.length > 0) throw new BatchDeletePartialError(result)
    return result
  },
}

// Cluster-scoped resources API
export const clusterResourceApi = {
  listAll: async <T extends K8sResource>(
    kind: ResourceKind,
    options: { limit?: number; continue?: string; silent?: boolean } = {}
  ): Promise<ListResponse<T>> => {
    const params = new URLSearchParams()
    if (options.limit) params.set('limit', String(options.limit))
    if (options.continue) params.set('continue', options.continue)
    const qs = params.toString() ? `?${params.toString()}` : ''
    const { data } = await apiClient.get(`/cluster/${kind}${qs}`, { _silent: options.silent } as any)
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
    target: ControllerMutationTarget,
    kind: ResourceKind,
    resource: T | string
  ): Promise<ApiResponse<string>> => {
    const content = typeof resource === 'string' ? resource : yaml.dump(resource)
    const config = await requireResourceMutation(target, kind, 'create')
    const { data } = await apiClient.post(`/cluster/${kind}`, content, {
      ...config,
      headers: { 'Content-Type': 'application/yaml' },
    })
    return data
  },

  update: async <T extends K8sResource>(
    target: ControllerMutationTarget,
    kind: ResourceKind,
    name: string,
    resource: T | string
  ): Promise<ApiResponse<string>> => {
    const content = typeof resource === 'string' ? resource : yaml.dump(resource)
    const config = await requireResourceMutation(target, kind, 'update')
    const { data } = await apiClient.put(`/cluster/${kind}/${name}`, content, {
      ...config,
      headers: { 'Content-Type': 'application/yaml', ...resourceVersionHeader(resource) },
    })
    return data
  },

  delete: async (target: ControllerMutationTarget, kind: ResourceKind, name: string, resourceVersion: string): Promise<ApiResponse<string>> => {
    const config = await requireResourceMutation(target, kind, 'delete')
    const { data } = await apiClient.delete(`/cluster/${kind}/${name}`, {
      ...config,
      headers: requiredResourceVersionHeader(resourceVersion),
    })
    return data
  },
}
