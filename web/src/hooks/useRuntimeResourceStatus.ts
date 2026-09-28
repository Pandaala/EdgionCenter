import { useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { resourceApi } from '@/api/resources'
import type { K8sResource, ResourceKind } from '@/api/types'
import { useControllerMutationTarget } from './useControllerMutationTarget'

const STATUS_QUERY = 'resource-runtime-status'
let activeReads = 0
const waiting: Array<() => void> = []

/** Bound visible-row observations independently of the paginated source list. */
async function withStatusSlot<T>(signal: AbortSignal, read: () => Promise<T>): Promise<T> {
  await new Promise<void>((resolve) => {
    const start = () => { activeReads++; resolve() }
    if (activeReads < 4) start()
    else waiting.push(start)
  })
  try {
    if (signal.aborted) throw new DOMException('Status read cancelled', 'AbortError')
    return await read()
  } finally {
    activeReads--
    waiting.shift()?.()
  }
}

export function useInvalidateRuntimeStatus(kind: ResourceKind, sourceUpdatedAt: number) {
  const client = useQueryClient()
  useEffect(() => {
    if (sourceUpdatedAt) {
      void client.invalidateQueries({ queryKey: [STATUS_QUERY, kind] }, { cancelRefetch: false })
    }
  }, [client, kind, sourceUpdatedAt])
}

/** Never return a processed spec to an editor or substitute a different version. */
export function useRuntimeResourceStatus(kind: ResourceKind, source: K8sResource) {
  const target = useControllerMutationTarget()
  const { name, namespace, resourceVersion } = source.metadata
  const hasSourceStatus = source.status !== undefined && source.status !== null
  return useQuery({
    queryKey: [STATUS_QUERY, kind, target.controllerId, namespace ?? '', name, resourceVersion],
    queryFn: ({ signal }) => withStatusSlot(signal, async () => {
      const observed = await resourceApi.getProcessed(target, kind, namespace, name, signal)
      const matches = observed.kind === source.kind
        && observed.metadata.name === name
        && (observed.metadata.namespace ?? '') === (namespace ?? '')
        && Boolean(resourceVersion)
        && observed.metadata.resourceVersion === resourceVersion
      return { matches, status: matches ? observed.status : undefined }
    }),
    enabled: !hasSourceStatus && Boolean(name && resourceVersion),
    retry: false,
    staleTime: 0,
    gcTime: 30_000,
    refetchInterval: hasSourceStatus ? false : 15_000,
  })
}
