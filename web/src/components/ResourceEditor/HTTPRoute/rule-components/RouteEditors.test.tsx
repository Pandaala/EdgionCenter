import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import RouteFiltersEditor, { switchRouteFilterType } from './RouteFiltersEditor'
import RulePoliciesEditor from './RulePoliciesEditor'

describe('structured route editors', () => {
  it('narrowly edits a header filter while preserving unknown fields and sibling rows', () => {
    const onChange = vi.fn()
    render(<RouteFiltersEditor value={[{
      type: 'RequestHeaderModifier',
      requestHeaderModifier: {
        set: [{ name: 'x-one', value: 'one' }, { name: 'x-two', value: 'two' }],
      },
      futureFilter: { retained: true },
    }]} onChange={onChange} />)

    fireEvent.change(screen.getByLabelText('set-value-0'), { target: { value: 'changed' } })

    expect(onChange).toHaveBeenCalledWith([{
      type: 'RequestHeaderModifier',
      requestHeaderModifier: {
        set: [{ name: 'x-one', value: 'changed' }, { name: 'x-two', value: 'two' }],
      },
      futureFilter: { retained: true },
    }])
  })

  it('narrowly edits timeout policy without replacing retry, session, or unknown fields', () => {
    const onChange = vi.fn()
    const rule = {
      timeouts: { request: '30s', backendRequest: '10s', futureTimeout: true },
      retry: { attempts: 3, codes: [502] },
      sessionPersistence: { type: 'Cookie' as const, strict: false },
      futureRule: { retained: true },
    }
    render(<RulePoliciesEditor value={rule} onChange={onChange} />)

    fireEvent.change(screen.getByPlaceholderText('30s'), { target: { value: '45s' } })

    expect(onChange).toHaveBeenCalledWith({
      ...rule,
      timeouts: { request: '45s', backendRequest: '10s', futureTimeout: true },
    })
  })

  it('preserves unknown filter fields while switching and drops only known payloads', () => {
    expect(switchRouteFilterType({
      type: 'RequestRedirect', requestRedirect: { scheme: 'https' }, futureFilter: { keep: true },
    }, 'URLRewrite')).toEqual({
      type: 'URLRewrite', urlRewrite: {}, futureFilter: { keep: true },
    })
  })
})
