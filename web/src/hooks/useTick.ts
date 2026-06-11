import { useEffect, useState } from 'react'

/**
 * Force re-render every `intervalMs` milliseconds.
 *
 * Use to refresh time-sensitive renders (e.g. relative timestamps).
 * In React 18 StrictMode the effect double-invokes; that's fine —
 * the interval is cleaned up properly.
 */
export function useTick(intervalMs: number): void {
  const [, setTick] = useState(0)
  useEffect(() => {
    const id = setInterval(() => setTick((n) => n + 1), intervalMs)
    return () => clearInterval(id)
  }, [intervalMs])
}
