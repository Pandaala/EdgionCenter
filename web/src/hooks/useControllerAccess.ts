import { useQuery } from '@tanstack/react-query'
import type { QueryClient } from '@tanstack/react-query'
import { controllerAccessApi, controllerKindFor, operationIsAllowed } from '@/api/access'
import type {
  ControllerAccessOperation,
  ControllerAccessResourceVerb,
  ResourceKind,
} from '@/api/types'

export const normalizeControllerAccessId = (controllerId: string | null) =>
  controllerId?.replace(/~/g, '/') ?? null

export const controllerAccessQueryKey = (controllerId: string | null) => [
  'controller-access',
  normalizeControllerAccessId(controllerId) ?? 'direct',
] as const

export function invalidateControllerAccess(queryClient: QueryClient, controllerId: string | null) {
  return queryClient.invalidateQueries({
    queryKey: controllerAccessQueryKey(controllerId),
    refetchType: 'all',
  })
}

export function useControllerAccess(controllerId: string | null, enabled = true) {
  const normalizedControllerId = normalizeControllerAccessId(controllerId)
  const query = useQuery({
    queryKey: controllerAccessQueryKey(normalizedControllerId),
    queryFn: () => controllerAccessApi.get(normalizedControllerId),
    staleTime: 10_000,
    refetchOnWindowFocus: true,
    retry: false,
    enabled,
  })
  // React Query intentionally retains old data during background fetches and
  // after refetch errors. Authorization must not use that stale snapshot.
  const accessIsUsable = enabled && query.isSuccess && !query.isFetching && !query.isError
  const accessDocument = accessIsUsable ? query.data : undefined

  return {
    ...query,
    data: accessDocument,
    authorizationPending: enabled && query.isFetching,
    canResource(resourceKind: ResourceKind, verb: ControllerAccessResourceVerb): boolean {
      if (!accessDocument) return false
      const controllerKind = controllerKindFor(resourceKind)
      return accessDocument.resources.find((row) => row.kind === controllerKind)?.verbs.includes(verb) === true
    },
    canOperation(operation: ControllerAccessOperation): boolean {
      return accessDocument ? operationIsAllowed(accessDocument, operation) : false
    },
  }
}
