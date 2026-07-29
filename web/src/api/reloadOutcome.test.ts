import { describe, expect, it } from 'vitest'
import { reloadErrorMessage, reloadOutcomeFromError, type ReloadOutcome } from './reloadOutcome'

function axiosLikeError(status: number, data: unknown) {
  return { isAxiosError: true, response: { status, data } }
}

describe('reloadOutcomeFromError', () => {
  /// `conflict` and `failed` are returned on a non-2xx status so scripts see an
  /// honest failure, which means axios rejects them. They are still outcomes
  /// and must reach the same rendering path as the 2xx states.
  it('lifts an outcome out of a rejected 409 and 503', () => {
    const conflict: ReloadOutcome = {
      controllerId: 'east/controller-a',
      state: 'conflict',
      reason: 'this Controller replica is a follower and refused the reload',
      leader: '10.0.0.7:12101',
    }
    expect(
      reloadOutcomeFromError(axiosLikeError(409, { success: false, data: conflict })),
    ).toEqual(conflict)

    const transient: ReloadOutcome = {
      controllerId: 'east/controller-a',
      state: 'failed',
      retryAfterSecs: 5,
    }
    expect(
      reloadOutcomeFromError(axiosLikeError(503, { success: false, data: transient })),
    ).toEqual(transient)
  })

  /// A transport failure (unknown controller, auth, network) has no outcome
  /// body and must keep propagating rather than being rendered as a reload
  /// verdict Center never reached the Controller to obtain.
  it('returns undefined for responses that carry no outcome', () => {
    expect(reloadOutcomeFromError(axiosLikeError(404, {
      success: false,
      error: 'Controller east/controller-a not found or offline',
    }))).toBeUndefined()
    expect(reloadOutcomeFromError(axiosLikeError(401, undefined))).toBeUndefined()
    expect(reloadOutcomeFromError(new Error('Network Error'))).toBeUndefined()
    expect(reloadOutcomeFromError(undefined)).toBeUndefined()
  })

  it('rejects a body whose data is not an outcome', () => {
    expect(reloadOutcomeFromError(axiosLikeError(502, { success: false, data: 'ok' })))
      .toBeUndefined()
    expect(reloadOutcomeFromError(axiosLikeError(502, { success: false, data: { state: 'failed' } })))
      .toBeUndefined()
  })
})

describe('reloadErrorMessage', () => {
  /// The reload request is `_silent`, so the interceptor's per-status message
  /// never fires and the caller must not fall back to axios's opaque
  /// "Request failed with status code 404".
  it('prefers the server error over axios wording', () => {
    expect(reloadErrorMessage(axiosLikeError(404, {
      success: false,
      error: 'Controller east/controller-a not found or offline',
    }))).toBe('Controller east/controller-a not found or offline')
  })

  it('falls back to the error message, then to a fixed string', () => {
    expect(reloadErrorMessage(new Error('Network Error'))).toBe('Network Error')
    // An empty server `error` must not win over a usable axios message.
    expect(reloadErrorMessage(Object.assign(
      new Error('Request failed with status code 502'),
      axiosLikeError(502, { success: false, error: '' }),
    ))).toBe('Request failed with status code 502')
    expect(reloadErrorMessage(undefined)).toBe('reload failed')
  })
})
