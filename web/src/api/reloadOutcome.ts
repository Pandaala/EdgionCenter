// ---------------------------------------------------------------------------
// Controller reload outcome (matches `center-app::api::reload_ops::ReloadOutcome`,
// camelCase on the wire).
//
// A Controller's own answer to a reload only ever means "queued" —
// `request_reload()` is fire-and-forget. Center therefore waits, bounded, for
// the Controller's `server_id` to change on the federation watch stream and
// reports a real terminal state. The states reuse the frozen ConfigData
// write vocabulary (`OutcomeState`) so both render through one component:
//
// - converged  — a new `server_id` was observed; the reload really finished.
// - accepted   — dispatched, but this Center replica cannot observe the
//                Controller's watch stream, so completion is unobservable here.
// - unknown    — dispatched, but no new `server_id` within the budget. The
//                Controller may still be restarting. NOT a success.
// - conflict   — a follower Controller refused it. Retrying this address is
//                pointless; `leader` carries where to send it instead.
// - failed     — rejected. `retryAfterSecs` present means the refusal was
//                transient and retrying is meaningful; absent means it is
//                not. It does not say WHY the Controller refused — the
//                Controller refuses transiently for more than one reason and
//                only `reason` carries which.
//
// `superseded` is part of the shared vocabulary but is never produced here —
// a reload overwrites no document.
// ---------------------------------------------------------------------------

import type { AxiosError } from 'axios'
import type { OutcomeState } from './writeOutcome'

export interface ReloadOutcome {
  controllerId: string
  state: OutcomeState
  /** Why the terminal state was reached. Absent on a clean `converged`. */
  reason?: string
  /** The new `server_id` observed. Present only on `converged` — its evidence. */
  serverId?: string
  /** Measured time from dispatch to observation. Telemetry only. */
  convergenceMs?: number
  /** Retry hint in seconds; present means the refusal was transient. */
  retryAfterSecs?: number
  /** Leader admin address from a `409 not leader`, when the Controller knew it. */
  leader?: string
}

interface ReloadOutcomeEnvelope {
  success: boolean
  data?: ReloadOutcome
  error?: string
}

function isReloadOutcome(value: unknown): value is ReloadOutcome {
  if (typeof value !== 'object' || value === null) return false
  const candidate = value as Partial<ReloadOutcome>
  return typeof candidate.state === 'string' && typeof candidate.controllerId === 'string'
}

/**
 * Lift the outcome out of an error response.
 *
 * `conflict` and `failed` are reported with a non-2xx status (409 / 503 / 502)
 * so a curl or a script sees an honest failure, which means axios rejects
 * them. They are still outcomes, not transport failures, so the caller must
 * be able to render them the same way as the 2xx states. Returns `undefined`
 * for a genuine transport/auth failure (404 unknown controller, 401, network
 * error), which has no outcome body and must keep propagating.
 */
export function reloadOutcomeFromError(error: unknown): ReloadOutcome | undefined {
  const body = (error as AxiosError<ReloadOutcomeEnvelope>)?.response?.data
  return isReloadOutcome(body?.data) ? body.data : undefined
}

/**
 * Best available message for a reload failure that produced no outcome — a
 * transport or auth failure that never reached the Controller.
 *
 * Prefers the server's own `error` (e.g. "Controller east/c0 not found or
 * offline") over axios's generic "Request failed with status code 404". The
 * reload request sets `_silent` so the shared interceptor does not bury the
 * outcome wording under its own toast, which also means the caller no longer
 * inherits the interceptor's per-status message and must word this itself.
 */
export function reloadErrorMessage(error: unknown): string {
  const serverError = (error as AxiosError<ReloadOutcomeEnvelope>)?.response?.data?.error
  if (typeof serverError === 'string' && serverError.length > 0) return serverError
  const message = (error as Error | undefined)?.message
  return typeof message === 'string' && message.length > 0 ? message : 'reload failed'
}
