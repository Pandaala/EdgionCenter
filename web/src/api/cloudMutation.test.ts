import { AxiosError, AxiosHeaders } from 'axios'
import { describe, expect, it } from 'vitest'
import { cloudMutationResult } from './cloudMutation'

describe('cloud mutation outcome observation', () => {
  it.each(['post', 'PUT', 'patch', 'delete'])('keeps a dispatched %s with no response ambiguous', (method) => {
    const error = new AxiosError('lost response', 'ERR_NETWORK', { method, headers: new AxiosHeaders() }, {})
    expect(cloudMutationResult(error)).toBe('ambiguous')
  })

  it('does not treat local guards, setup failures or failed reads as dispatched mutations', () => {
    expect(cloudMutationResult(new Error('unsafe_zone_delete'))).toBe('rejected')
    expect(cloudMutationResult(new AxiosError('invalid setup', 'ERR_BAD_OPTION', { method: 'post', headers: new AxiosHeaders() }))).toBe('rejected')
    expect(cloudMutationResult(new AxiosError('read timed out', 'ECONNABORTED', { method: 'get', headers: new AxiosHeaders() }, {}))).toBe('rejected')
    expect(cloudMutationResult(null)).toBe('rejected')
  })

  it('distinguishes explicit rejection, conflict and server-reported uncertainty', () => {
    expect(cloudMutationResult({ response: { status: 403, data: { error: 'forbidden' } } })).toBe('rejected')
    expect(cloudMutationResult({ response: { status: 412, data: { error: 'conflict' } } })).toBe('conflicted')
    expect(cloudMutationResult({ response: { status: 503, data: { error: 'unknown_outcome' } } })).toBe('ambiguous')
  })
})
