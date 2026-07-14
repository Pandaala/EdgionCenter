import { useQuery } from '@tanstack/react-query'
import { systemApi } from '@/api/client'

export type AuthzMode = 'allow_all' | 'rbac'

/**
 * Fetches the public `/server-info` discovery document once and caches it.
 *
 * The dashboard derives login, menu, and route availability from the explicit
 * capability flags. `authzMode` and `dbAuthEnabled` remain descriptive metadata
 * for compatibility and must not be used to infer whether a feature exists.
 * This hook keys the query as `['server-info']` — a separate cached entry from
 * the dashboards, which use `['server-info', controllerId ?? '']`. It is
 * therefore NOT deduped with them; with `staleTime: Infinity` it is fetched
 * once for the app shell.
 */
export function useServerInfo() {
  return useQuery({
    queryKey: ['server-info'],
    queryFn: systemApi.serverInfo,
    staleTime: Infinity,
  })
}
