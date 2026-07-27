// ---------------------------------------------------------------------------
// Shared ConfigData write-outcome contract (frozen — matches
// `center-app::api::config_data_ops::WriteOutcome`, camelCase on the wire).
//
// Every Center-initiated write to an EdgionConfigData resource (RegionRoute
// failover, row-level override sync) reports one outcome per target
// Controller through this shape. The six states are not interchangeable:
//
// - converged  — the write landed and the intent is currently in effect.
// - superseded — the write landed and was then overwritten by someone else;
//                `observed` carries the document actually in effect now.
//                NOT a failure and NOT a timeout.
// - accepted   — the write landed, but this Center replica does not hold
//                that Controller's federation session, so it cannot observe
//                convergence locally.
// - conflict   — the CAS precondition was rejected; nothing was applied.
//                The operator's view was stale — refresh and retry.
// - failed     — the write was rejected.
// - unknown    — the write landed but convergence was not observed within
//                the budget.
// ---------------------------------------------------------------------------

export type OutcomeState =
  | 'converged'
  | 'superseded'
  | 'accepted'
  | 'conflict'
  | 'failed'
  | 'unknown'

export interface WriteOutcome {
  controllerId: string
  state: OutcomeState
  /** Why the terminal state was reached. Absent on a clean `converged`. */
  reason?: string
  /** The document as last observed in the local cache. Present for `superseded`. */
  observed?: unknown
  /** Measured time from write dispatch to observation. Telemetry only. */
  convergenceMs?: number
}

/** Aggregate response shape for a fan-out write through the shared write core. */
export interface WriteOutcomeSummary {
  modified: number
  failed: number
  outcomes: WriteOutcome[]
}

/**
 * `failed` and `conflict` are the only states where nothing landed on that
 * controller. `converged`/`superseded`/`accepted`/`unknown` all mean the
 * write was applied — see the module doc comment on `WriteOutcome`.
 */
export function isOutcomeFailure(state: OutcomeState): boolean {
  return state === 'failed' || state === 'conflict'
}
