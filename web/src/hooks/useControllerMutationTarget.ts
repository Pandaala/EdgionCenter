import { useMemo } from 'react'
import { useParams } from 'react-router-dom'
import type { ControllerMutationTarget } from '@/api/resources'

/**
 * Capture the route's Controller target during render. Mutations retain this
 * immutable value even if navigation changes the process-global proxy state
 * before a confirmation callback or async mutation actually runs.
 */
export function useControllerMutationTarget(): ControllerMutationTarget {
  const { controllerId } = useParams<{ controllerId?: string }>()
  return useMemo(
    () => ({ controllerId: controllerId?.replace(/~/g, '/') ?? null }),
    [controllerId],
  )
}
