import { createContext, createElement, Fragment, useContext, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { authApi } from '../api/auth'

/**
 * Permission context value.
 *
 * `permissions` is the list of keys the backend granted the current user
 * (fetched once from `/auth/me` after login). In the LITE tier this is the full
 * catalog, so every `useCan` check passes. `loading` is true until the first
 * `/auth/me` response (or failure) settles.
 */
interface PermissionContextValue {
  permissions: string[]
  loading: boolean
}

const PermissionContext = createContext<PermissionContextValue>({
  permissions: [],
  loading: true,
})

/**
 * Fetches `/auth/me` once on mount and exposes the caller's permission keys to
 * the subtree. Wrap the authenticated app in this provider so menus and direct
 * routes enforce the same permission set.
 */
export function PermissionProvider({ children }: { children: ReactNode }) {
  const [permissions, setPermissions] = useState<string[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    authApi
      .me()
      .then((res) => {
        if (cancelled) return
        setPermissions(res.data?.permissions ?? [])
      })
      .catch(() => {
        if (cancelled) return
        setPermissions([])
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  return createElement(PermissionContext.Provider, { value: { permissions, loading } }, children)
}

/** Returns true when the current user holds `key`. */
export function useCan(key: string): boolean {
  const { permissions } = useContext(PermissionContext)
  return permissions.includes(key)
}

/** Access the raw permission list and loading state. */
export function usePermissions(): PermissionContextValue {
  return useContext(PermissionContext)
}

export function PermissionGate({
  permission,
  children,
  pending = null,
  denied = null,
}: {
  permission: string
  children: ReactNode
  pending?: ReactNode
  denied?: ReactNode
}) {
  const { permissions, loading } = usePermissions()
  if (loading) return createElement(Fragment, null, pending)
  if (!permissions.includes(permission)) return createElement(Fragment, null, denied)
  return createElement(Fragment, null, children)
}
