import type { ResourceKind } from '@/api/types'
import { getResourceCatalogEntry, type MutationPathSegment } from '@/config/resourceCatalog'
import * as yaml from 'js-yaml'

type JsonRecord = Record<string, unknown>
export type DocumentPathSegment = MutationPathSegment

export interface MutationDocumentOptions {
  mode: 'create' | 'update'
  /** Selects a complete catalog boundary; missing/unknown kinds fail closed. */
  resourceKind: ResourceKind
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function cloneValue<T>(value: T): T {
  if (Array.isArray(value)) {
    return value.map((item) => cloneValue(item)) as T
  }
  if (isRecord(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, cloneValue(item)]),
    ) as T
  }
  return value
}

/**
 * Apply defaults to a new create draft. Never use this while parsing an API
 * response: existing resources must not gain frontend defaults. Values already
 * present in the draft always win, including null, false, zero, empty strings,
 * and empty arrays.
 */
export function withCreateDefaults<T>(raw: unknown, defaults: T): T {
  if (raw === undefined) return cloneValue(defaults)
  if (!isRecord(raw) || !isRecord(defaults)) return cloneValue(raw) as T

  const result: JsonRecord = cloneValue(defaults)
  for (const [key, rawValue] of Object.entries(raw)) {
    const defaultValue = (defaults as JsonRecord)[key]
    result[key] = isRecord(rawValue) && isRecord(defaultValue)
      ? withCreateDefaults(rawValue, defaultValue)
      : cloneValue(rawValue)
  }
  return result as T
}

/**
 * Immutably replace one value in a resource document. Array indexes are valid
 * path segments. Siblings and unknown fields are retained verbatim.
 */
export function setDocumentValue<T>(
  document: T,
  path: ReadonlyArray<string | number>,
  value: unknown,
): T {
  if (path.length === 0) return cloneValue(value) as T

  const root = cloneValue(document) as unknown
  let cursor: unknown = root

  path.forEach((segment, index) => {
    if (!isRecord(cursor) && !Array.isArray(cursor)) {
      throw new Error(`Cannot traverse resource document at ${path.slice(0, index).join('.')}`)
    }

    const isLast = index === path.length - 1
    if (isLast) {
      const container = cursor as Record<string | number, unknown>
      container[segment] = cloneValue(value)
      return
    }

    const nextSegment = path[index + 1]
    const container = cursor as Record<string | number, unknown>
    const existing = container[segment]
    if (!isRecord(existing) && !Array.isArray(existing)) {
      container[segment] = typeof nextSegment === 'number' ? [] : {}
    }
    cursor = container[segment]
  })

  return root as T
}

/**
 * Immutably shallow-patch an object at a resource path while preserving every
 * unmentioned key in that object and elsewhere in the document.
 */
export function patchDocumentObject<T>(
  document: T,
  path: ReadonlyArray<string | number>,
  patch: JsonRecord,
): T {
  let current: unknown = document
  for (const segment of path) {
    if (!isRecord(current) && !Array.isArray(current)) {
      throw new Error(`Cannot patch non-object resource value at ${path.join('.')}`)
    }
    current = (current as Record<string | number, unknown>)[segment]
  }

  if (current !== undefined && !isRecord(current)) {
    throw new Error(`Cannot patch non-object resource value at ${path.join('.')}`)
  }

  return setDocumentValue(document, path, { ...(current ?? {}), ...patch })
}

function deleteAtPath(
  value: unknown,
  path: ReadonlyArray<DocumentPathSegment>,
  index: number,
): void {
  if (index >= path.length || (!isRecord(value) && !Array.isArray(value))) return
  const segment = path[index]
  const isLast = index === path.length - 1

  if (segment === '*') {
    if (isLast && Array.isArray(value)) {
      value.splice(0)
      return
    }
    for (const key of Object.keys(value)) {
      if (isLast) {
        delete (value as Record<string, unknown>)[key]
      } else {
        deleteAtPath((value as Record<string, unknown>)[key], path, index + 1)
      }
    }
    return
  }

  if (segment === '**') {
    if (isLast) throw new Error('Recursive descent must name a terminal field')
    deleteAtPath(value, path, index + 1)
    for (const key of Object.keys(value)) {
      deleteAtPath((value as Record<string, unknown>)[key], path, index)
    }
    return
  }

  const container = value as Record<string | number, unknown>
  if (isLast) {
    if (Array.isArray(value) && typeof segment === 'number') value.splice(segment, 1)
    else delete container[segment]
    return
  }
  deleteAtPath(container[segment], path, index + 1)
}

/** Clone a document and remove exact paths. `*` matches every object key or array entry. */
export function withoutDocumentPaths<T>(
  document: T,
  paths: ReadonlyArray<ReadonlyArray<DocumentPathSegment>>,
): T {
  const result = cloneValue(document)
  paths.forEach((path) => deleteAtPath(result, path, 0))
  return result
}

/**
 * Build the common operator mutation envelope without projecting the spec onto
 * frontend-known fields. Server metadata and status are never sent back.
 */
export function buildMutationDocument(
  document: unknown,
  options: MutationDocumentOptions,
): JsonRecord {
  if (!isRecord(document) || !isRecord(document.metadata)) {
    throw new Error('Resource document must contain metadata')
  }
  const boundary = getResourceCatalogEntry(options.resourceKind)
  if (document.kind !== boundary.displayName) {
    throw new Error(
      `Resource document kind ${String(document.kind)} does not match ${boundary.displayName}`,
    )
  }
  const acceptedApiVersions = [boundary.apiVersion, ...(boundary.acceptedApiVersions ?? [])]
  if (typeof document.apiVersion !== 'string' || !acceptedApiVersions.includes(document.apiVersion)) {
    throw new Error(
      `Resource document apiVersion ${String(document.apiVersion)} is not supported for ${boundary.displayName}`,
    )
  }
  const metadata = document.metadata
  const operatorMetadata: JsonRecord = {}
  const metadataKeys = options.mode === 'update'
    ? ['name', 'namespace', 'labels', 'annotations', 'resourceVersion']
    : ['name', 'namespace', 'labels', 'annotations']
  for (const key of metadataKeys) {
    if (metadata[key] !== undefined) operatorMetadata[key] = cloneValue(metadata[key])
  }

  const mutation: JsonRecord = {
    apiVersion: cloneValue(document.apiVersion),
    kind: cloneValue(document.kind),
    metadata: operatorMetadata,
  }
  const protectedTopLevelFields = new Set(['apiVersion', 'kind', 'metadata', 'status'])
  for (const field of boundary.operatorTopLevelFields) {
    if (protectedTopLevelFields.has(field)) {
      throw new Error(`Operator top-level field is protected: ${field}`)
    }
    if (document[field] !== undefined) mutation[field] = cloneValue(document[field])
  }

  // The mode is intentionally explicit even though the common envelope is the
  // same today; resource adapters can layer create/update-specific validation.
  if (options.mode !== 'create' && options.mode !== 'update') {
    throw new Error('Unsupported mutation mode')
  }
  return withoutDocumentPaths(mutation, boundary.excludedMutationPaths)
}

/** Serialize the catalog-bound operator mutation document without deleting empties. */
export function mutationDocumentToYaml(
  document: unknown,
  resourceKind: ResourceKind,
  mode: 'create' | 'update',
): string {
  return yaml.dump(buildMutationDocument(document, { resourceKind, mode }), {
    lineWidth: -1,
    noRefs: true,
  })
}
