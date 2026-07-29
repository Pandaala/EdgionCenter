import type { ReloadOutcome } from '@/api/reloadOutcome'

/**
 * How a reload outcome must be surfaced. `success` is a dismissable toast;
 * `warning`/`error` are modals, because every one of those states carries
 * something the operator has to read and act on (a leader address, a retry
 * delay, or "this may not have finished") and a 3 s toast loses it.
 */
export type ReloadOutcomeLevel = 'success' | 'warning' | 'error'

export interface ReloadOutcomeMessage {
  level: ReloadOutcomeLevel
  text: string
}

/**
 * Operator-facing wording for one reload outcome.
 *
 * `converged` is the ONLY state that may claim the reload finished — the whole
 * point of the outcome is that the Controller's `200` means "queued", so
 * `accepted`/`unknown` must read as unconfirmed rather than as success.
 *
 * `conflict` (a follower refused it) and a `failed` carrying `retryAfterSecs`
 * (a transient refusal) get their own wording rather than the generic
 * ConfigData write text: one is "retrying is pointless, go here instead" and
 * the other is "retrying is exactly right, in N seconds". They used to
 * collapse into the same error toast.
 */
export function reloadOutcomeMessage(
  t: (key: string, params?: Record<string, string | number>) => string,
  outcome: ReloadOutcome,
): ReloadOutcomeMessage {
  const name = outcome.controllerId
  switch (outcome.state) {
    case 'converged':
      return {
        level: 'success',
        text: t('center.reloadOutcome.converged', {
          name,
          serverId: outcome.serverId ?? '—',
          seconds: formatSeconds(outcome.convergenceMs),
        }),
      }
    case 'accepted':
      return { level: 'warning', text: t('center.reloadOutcome.accepted', { name }) }
    // Grouped with `unknown` deliberately: `superseded` cannot be produced by
    // the reload path, and if a future change ever did produce it, reading as
    // "unconfirmed" is the safe default — never as success.
    case 'superseded':
    case 'unknown':
      return {
        level: 'warning',
        text: t('center.reloadOutcome.unknown', { name, seconds: RELOAD_BUDGET_SECONDS }),
      }
    case 'conflict':
      return {
        level: 'error',
        text: outcome.leader
          ? t('center.reloadOutcome.conflict', { name, leader: outcome.leader })
          : t('center.reloadOutcome.conflictNoLeader', { name }),
      }
    case 'failed':
      // A retry hint means the Controller refused transiently. It refuses for
      // more than one reason (a reload already queued, or a config center not
      // ready yet) and only its own message says which, so `reason` is shown
      // rather than paraphrased — the retry action is the same either way, but
      // the diagnosis is not.
      return outcome.retryAfterSecs != null
        ? {
            level: 'warning',
            text: t('center.reloadOutcome.retry', {
              name,
              seconds: outcome.retryAfterSecs,
              reason: outcome.reason ?? '—',
            }),
          }
        : {
            level: 'error',
            text: t('center.reloadOutcome.failed', {
              name,
              reason: outcome.reason ?? '—',
            }),
          }
  }
}

/**
 * Mirrors `center-app::api::reload_ops::RELOAD_BUDGET`. Only used to word the
 * `unknown` message, which is reached precisely when the backend gave up after
 * this many seconds.
 */
const RELOAD_BUDGET_SECONDS = 20

function formatSeconds(milliseconds?: number): string {
  if (milliseconds == null) return '—'
  return (milliseconds / 1000).toFixed(1)
}
