import { useEffect, useMemo, useRef } from 'react'
import { useInfiniteQuery, useQueryClient } from '@tanstack/react-query'
import { message } from 'antd'
import { resourceApi, clusterResourceApi } from '@/api/resources'
import type { K8sResource, ResourceKind } from '@/api/types'
import { useT } from '@/i18n'

const DEFAULT_PAGE_SIZE = 50
const STALE_RESET_DEDUP_MS = 5000

interface UseResourceListOptions {
  /** True for namespaced kinds; false for cluster-scoped kinds. */
  namespaced: boolean
  /** Optional namespace filter (namespaced kinds only). */
  namespace?: string
  /** Page size, default 50. */
  limit?: number
  /**
   * Optional extra cache-key segment. Pass e.g. controllerId to scope
   * cache by controller. Default null.
   */
  scope?: string | null
  /** Fail-closed authorization gate. No request is issued while false. */
  enabled?: boolean
}

function isStalePaginationError(err: unknown): boolean {
  if (!err || typeof err !== 'object') return false
  const anyErr = err as { response?: { status?: number; data?: { code?: string } } }
  if (anyErr.response?.status === 410) return true
  if (anyErr.response?.data?.code === 'StalePagination') return true
  return false
}

/**
 * Wraps useInfiniteQuery for listAll cursor pagination. Returns a flat
 * `items` array (already concatenated across pages) plus React Query state.
 *
 * Stale-token recovery: if backend returns HTTP 410 or `code: StalePagination`,
 * the hook auto-removes its cached queries (triggering a fresh first-page
 * fetch) and shows an info toast. A 5-second dedup window prevents toast
 * spam if the backend stays stale.
 */
export function useResourceList<T extends K8sResource>(
  kind: ResourceKind,
  options: UseResourceListOptions,
) {
  const { namespaced, namespace, limit = DEFAULT_PAGE_SIZE, scope = null, enabled = true } = options
  const t = useT()
  const queryClient = useQueryClient()
  const lastResetRef = useRef<number>(0)

  const queryKey = useMemo(
    () => ['resource-list', kind, namespaced ? namespace ?? null : 'cluster', limit, scope],
    [kind, namespaced, namespace, limit, scope],
  )

  const query = useInfiniteQuery({
    queryKey,
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }) => {
      if (namespaced) {
        if (namespace) {
          return resourceApi.list<T>(kind, namespace) // single-namespace path doesn't support cursor; one page
        }
        return resourceApi.listAll<T>(kind, { limit, continue: pageParam })
      }
      return clusterResourceApi.listAll<T>(kind, { limit, continue: pageParam })
    },
    getNextPageParam: (lastPage) => lastPage.continue_token ?? undefined,
    enabled,
  })

  // Stale-token auto-recovery with 5s dedup window.
  useEffect(() => {
    if (!query.error) return
    if (!isStalePaginationError(query.error)) return
    const now = Date.now()
    if (now - lastResetRef.current < STALE_RESET_DEDUP_MS) return
    lastResetRef.current = now
    // Must match the exact queryKey used by useInfiniteQuery above
    // (undefined !== null in react-query v5 hashKey).
    queryClient.removeQueries({ queryKey })
    message.info(t('msg.tokenExpiredRefreshed'))
  }, [query.error, queryClient, queryKey, t])

  const items = useMemo(
    () => (query.data?.pages ?? []).flatMap((p) => p.data ?? []),
    [query.data],
  )

  return {
    items,
    isLoading: query.isLoading,
    error: query.error,
    refetch: query.refetch,
    fetchNextPage: query.fetchNextPage,
    hasNextPage: query.hasNextPage,
    isFetchingNextPage: query.isFetchingNextPage,
  }
}
