import { useParams } from 'react-router-dom'
import { useLayoutEffect } from 'react'
import { setActiveControllerId } from '@/utils/proxy'
import { AppShell } from '../shell/AppShell'

export default function ControllerProxy() {
  const { controllerId } = useParams<{ controllerId: string }>()

  // URL uses "~" instead of "/" in controller_id (browser decodes %2F).
  // Restore to real id for display/API usage; the proxy interceptor
  // in client.ts converts back to "~" when building proxy URLs.
  const realId = controllerId?.replace(/~/g, '/') ?? null

  // useLayoutEffect (not useEffect) so activeControllerId is set
  // before React Query's useEffect fires queryFn. With useEffect the
  // cleanup from the previous render ran after commit but before
  // React Query executed, leaving activeControllerId = null and
  // sending requests to center instead of the target controller.
  useLayoutEffect(() => {
    setActiveControllerId(realId)
    return () => {
      setActiveControllerId(null)
    }
  }, [realId])

  return <AppShell />
}
