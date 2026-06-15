import { useQuery } from '@tanstack/react-query'
import { systemApi } from '@/api/client'

export type AuthzMode = 'allow_all' | 'rbac'

/**
 * Fetches `/server-info` once (cached) and exposes the access-control fields.
 *
 * Access control is orthogonal: `authzMode` (`allow_all` | `rbac`) is the
 * authorization model, and `dbAuthEnabled` reports whether DB-backed (table
 * `users`) authentication is in use. The dashboard derives menu visibility from
 * these — under `allow_all` the backend grants every permission to everyone, so
 * the permission keys alone cannot tell whether user/role management is real.
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
