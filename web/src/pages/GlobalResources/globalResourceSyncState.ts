import type { GlobalResourceComparisonMember, WatchSyncState } from '@/api/globalResources'

const SYNC_STATE_SEVERITY: Record<WatchSyncState, number> = {
  ok: 0,
  stale: 1,
  overflowed: 2,
}

export const SYNC_STATE_TAG_COLOR: Record<WatchSyncState, string> = {
  ok: 'green',
  stale: 'orange',
  overflowed: 'red',
}

/**
 * Group-level worst sync state across members: overflowed > stale > ok.
 * Members without a syncState (older Controllers, or fields not yet populated)
 * count as ok.
 */
export function getWorstSyncState(
  members: readonly Pick<GlobalResourceComparisonMember, 'syncState'>[],
): WatchSyncState {
  let worst: WatchSyncState = 'ok'
  for (const member of members) {
    const state = member.syncState ?? 'ok'
    if (SYNC_STATE_SEVERITY[state] > SYNC_STATE_SEVERITY[worst]) {
      worst = state
    }
  }
  return worst
}
