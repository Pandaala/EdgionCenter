import dayjs from 'dayjs'
import { useTick } from '@/hooks/useTick'

interface AgeCellProps {
  timestamp?: string
}

const TICK_MS = 60_000

/**
 * Renders a self-refreshing relative timestamp (e.g. "5 minutes ago").
 * Re-renders every 60s so the displayed age stays in sync without
 * needing to refetch the list. Returns '-' when timestamp is missing.
 */
export default function AgeCell({ timestamp }: AgeCellProps) {
  useTick(TICK_MS)
  if (!timestamp) return <>-</>
  return <>{dayjs(timestamp).fromNow()}</>
}
