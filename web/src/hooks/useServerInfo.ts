import { useQuery } from '@tanstack/react-query'
import { systemApi } from '@/api/client'

export type AccessMode = 'lite' | 'full'

/**
 * Fetches `/server-info` once (cached) and exposes the access-control tier.
 *
 * `accessMode` is the single source of truth the dashboard uses to know whether
 * user/role management should be available: in `lite` the backend's
 * `AllowAllAuthz` grants `users:manage` / `roles:manage` to everyone, so the
 * permission keys alone cannot distinguish `lite` from `full`. React Query
 * dedupes this against the same query key used elsewhere.
 */
export function useServerInfo() {
  return useQuery({
    queryKey: ['server-info'],
    queryFn: systemApi.serverInfo,
    staleTime: Infinity,
  })
}
