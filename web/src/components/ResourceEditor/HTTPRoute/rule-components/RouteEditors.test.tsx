import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import RouteFiltersEditor, { switchRouteFilterType } from './RouteFiltersEditor'
import RulePoliciesEditor from './RulePoliciesEditor'

describe('structured route editors', () => {
  it.each(['http', 'grpc'] as const)('omits cleared %s policy fields without erasing configured blocks or siblings', (protocol) => {
    const onChange = vi.fn()
    const rule = {
      timeouts: { request: '30s', backendRequest: '10s', future: false },
      retry: { attempts: 0, backoff: '1s', codes: protocol === 'http' ? [503] : [14] },
      sessionPersistence: { type: 'Cookie' as const, sessionName: 'SESSION', absoluteTimeout: '1h', strict: false },
      futureRule: [],
    }
    render(<RulePoliciesEditor value={rule} onChange={onChange} protocol={protocol} />)
    for (const [label, section, key] of [
      ['Request Timeout', 'timeouts', 'request'],
      ['Backend Request Timeout', 'timeouts', 'backendRequest'],
      ['Backoff', 'retry', 'backoff'],
      ['Session Name', 'sessionPersistence', 'sessionName'],
      ['Absolute Timeout', 'sessionPersistence', 'absoluteTimeout'],
    ] as const) {
      fireEvent.change(screen.getByRole('textbox', { name: label }), { target: { value: '' } })
      const expected: Record<string, unknown> = { ...rule[section] }
      delete expected[key]
      expect(onChange).toHaveBeenLastCalledWith({ ...rule, [section]: expected })
    }
    expect(screen.queryByText('Idle Timeout', { exact: true })).not.toBeInTheDocument()
    expect(screen.getByText(/Strict session persistence is supported with FileSystem/)).toBeInTheDocument()
  })

  it('preserves an unsupported idleTimeout until explicitly removed and respects read-only mode', () => {
    const onChange = vi.fn()
    const rule = { sessionPersistence: { idleTimeout: '20s', strict: false, future: [] } }
    const { rerender } = render(<RulePoliciesEditor value={rule} onChange={onChange} />)
    expect(screen.getByText(/does not implement session idleTimeout/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    expect(onChange).toHaveBeenCalledWith({ sessionPersistence: { strict: false, future: [] } })
    rerender(<RulePoliciesEditor value={rule} onChange={onChange} disabled />)
    expect(screen.queryByRole('button', { name: 'Clear' })).not.toBeInTheDocument()
  })

  it('keeps an explicit empty override block when its last field is cleared', () => {
    const onChange = vi.fn()
    render(<RulePoliciesEditor value={{ timeouts: { request: '30s' } }} onChange={onChange} />)
    fireEvent.change(screen.getByRole('textbox', { name: 'Request Timeout' }), { target: { value: '' } })
    expect(onChange).toHaveBeenCalledWith({ timeouts: {} })
  })
  it.each([
    ['http', 'HTTP Retry Status Codes (400-599)', '200', [503]],
    ['http', 'HTTP Retry Status Codes (400-599)', '429', [503, 429]],
    ['grpc', 'gRPC Retry Status Codes (0-16)', '14', [14]],
  ] as const)('limits %s retry choices when entering %s / %s', (protocol, label, entered, expected) => {
    const onChange = vi.fn()
    const rule = { retry: { codes: protocol === 'http' ? [503] : [], attempts: 2 }, future: false }
    render(<RulePoliciesEditor value={rule} onChange={onChange} protocol={protocol} />)
    const input = screen.getByRole('combobox', { name: label })
    fireEvent.mouseDown(input)
    fireEvent.change(input, { target: { value: entered } })
    fireEvent.keyDown(input, { key: 'Enter', keyCode: 13 })
    expect(onChange).toHaveBeenLastCalledWith({ ...rule, retry: { ...rule.retry, codes: [...expected] } })
  })
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

  it('edits the Gateway API RequestMirror percent field without inline tuning controls', () => {
    const onChange = vi.fn()
    render(<RouteFiltersEditor value={[{
      type: 'RequestMirror',
      requestMirror: {
        backendRef: { name: 'mirror' },
        fraction: { numerator: 1, denominator: 2 },
        percent: 25,
        percentage: 10,
        connectTimeoutMs: 500,
        futureMirrorField: { retained: true },
      } as any,
    }]} onChange={onChange} />)

    fireEvent.change(screen.getByLabelText('Mirror Percent'), { target: { value: '40' } })

    expect(onChange).toHaveBeenCalledWith([{
      type: 'RequestMirror',
      requestMirror: {
        backendRef: { name: 'mirror' },
        percent: 40,
        futureMirrorField: { retained: true },
      },
    }])
    expect(screen.queryByText('Connect Timeout (ms)')).not.toBeInTheDocument()
    expect(screen.queryByText('Dedicated Mirror Access Log')).not.toBeInTheDocument()
  })
})
