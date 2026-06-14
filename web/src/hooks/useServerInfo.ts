import { useQuery } from '@tanstack/react-query'
import { systemApi } from '@/api/client'

export type AccessMode = 'lite' | 'full'

/**
 * Fetches `/server-info` once (cached) and exposes the access-control tier.
 *
 * `accessMode` is the single source of truth the dashboard uses to know whether
 * user/role management should be available: in `lite` the backend's
 * `AllowAllAuthz` grants `users:manage` / `roles:manage` to everyone, so the
 * permission keys alone cannot distinguish `lite` from `full`. This hook keys
 * the query as `['server-info']` — a separate cached entry from the dashboards,
 * which use `['server-info', controllerId ?? '']`. It is therefore NOT deduped
 * with them; with `staleTime: Infinity` it is fetched once for the app shell.
 */
export function useServerInfo() {
  return useQuery({
    queryKey: ['server-info'],
    queryFn: systemApi.serverInfo,
    staleTime: Infinity,
  })
}
