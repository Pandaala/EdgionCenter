import type { APIRequestContext } from '@playwright/test'

export interface ResourceSnapshot { generation?: number; resourceVersion?: string; spec: unknown; conditions: unknown[] }

function resourcePath(kind: string, scope: 'Namespaced' | 'Cluster', namespace: string | undefined, name: string): string {
  return scope === 'Cluster'
    ? `/cluster/${encodeURIComponent(kind)}/${encodeURIComponent(name)}`
    : `/namespaced/${encodeURIComponent(kind)}/${encodeURIComponent(namespace!)}/${encodeURIComponent(name)}`
}

export async function readControllerResourceDocument(
  request: APIRequestContext,
  controller: string,
  kind: string,
  scope: 'Namespaced' | 'Cluster',
  namespace: string | undefined,
  name: string,
): Promise<Record<string, any>> {
  const safeController = controller.replaceAll('/', '~')
  const response = await request.get(`/api/v1/proxy/${safeController}/api/v1${resourcePath(kind, scope, namespace, name)}`)
  if (!response.ok()) throw new Error(`API document oracle failed: ${response.status()} ${await response.text()}`)
  return response.json() as Promise<Record<string, any>>
}

function collectConditions(status: any): unknown[] {
  return [
    ...(Array.isArray(status?.conditions) ? status.conditions : []),
    ...(Array.isArray(status?.parents) ? status.parents.flatMap((parent: any) => Array.isArray(parent?.conditions) ? parent.conditions : []) : []),
    ...(Array.isArray(status?.listeners) ? status.listeners.flatMap((listener: any) => Array.isArray(listener?.conditions) ? listener.conditions : []) : []),
    ...(Array.isArray(status?.ancestors) ? status.ancestors.flatMap((ancestor: any) => Array.isArray(ancestor?.conditions) ? ancestor.conditions : []) : []),
  ]
}

export async function readControllerResource(request: APIRequestContext, controller: string, kind: string, scope: 'Namespaced' | 'Cluster', namespace: string | undefined, name: string): Promise<ResourceSnapshot> {
  const safeController = controller.replaceAll('/', '~')
  const response = await request.get(`/api/v1/proxy/${safeController}/api/v1${resourcePath(kind, scope, namespace, name)}`)
  if (!response.ok()) throw new Error(`API oracle failed: ${response.status()} ${await response.text()}`)
  const value = await response.json()
  return { generation: value.metadata?.generation, resourceVersion: value.metadata?.resourceVersion, spec: value.spec, conditions: collectConditions(value.status) }
}

/**
 * Read the Controller's processed ConfigSync cache rather than the backing
 * configuration store. In file-system mode native status is persisted in a
 * sibling `.status` file, so the regular CRUD endpoint intentionally returns
 * the operator-owned spec without the Controller-computed status. The
 * ConfigSync cache contains the processed object that produced that sidecar
 * (and is also the authoritative processed view in Kubernetes mode).
 */
export async function readControllerProcessedResource(
  request: APIRequestContext,
  controller: string,
  kind: string,
  namespace: string | undefined,
  name: string,
): Promise<ResourceSnapshot> {
  const safeController = controller.replaceAll('/', '~')
  const query = new URLSearchParams({ name })
  if (namespace) query.set('namespace', namespace)
  const response = await request.get(`/api/v1/proxy/${safeController}/configserver/${encodeURIComponent(kind)}?${query}`)
  if (!response.ok()) throw new Error(`Processed API oracle failed: ${response.status()} ${await response.text()}`)
  const envelope = await response.json() as { success?: boolean; data?: any }
  if (envelope.success !== true || !envelope.data) throw new Error('Processed API oracle returned an invalid response envelope')
  const value = envelope.data
  return { generation: value.metadata?.generation, resourceVersion: value.metadata?.resourceVersion, spec: value.spec, conditions: collectConditions(value.status) }
}

export async function pollResource(request: APIRequestContext, read: () => Promise<ResourceSnapshot>, predicate: (snapshot: ResourceSnapshot) => boolean, timeoutMs = 20_000): Promise<ResourceSnapshot> {
  const deadline = Date.now() + timeoutMs; let last: ResourceSnapshot | undefined; let lastError: unknown
  while (Date.now() < deadline) {
    try {
      last = await read(); lastError = undefined
      if (predicate(last)) return last
    } catch (error) {
      // ConfigSync registration and the processed cache become available
      // asynchronously. Retry bounded transient 404/503/network failures, but
      // retain the last error so a permanently broken oracle still fails loud.
      lastError = error
    }
    await new Promise((resolve) => setTimeout(resolve, 200))
  }
  const detail = lastError instanceof Error ? lastError.message : JSON.stringify(last)
  throw new Error(`API oracle deadline exceeded: ${detail}`)
}
