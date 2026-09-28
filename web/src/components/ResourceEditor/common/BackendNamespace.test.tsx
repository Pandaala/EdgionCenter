import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import HTTPBackendRefsEditor from '../HTTPRoute/rule-components/BackendRefsEditor'
import GRPCBackendRefsEditor from '../GRPCRoute/sections/GRPCBackendRefsEditor'
import StreamBackendRefsEditor from './BackendRefsEditor'

vi.mock('../HTTPRoute/rule-components/RouteFiltersEditor', () => ({ default: () => null }))

describe.each([
  ['HTTP', HTTPBackendRefsEditor],
  ['gRPC', GRPCBackendRefsEditor],
  ['stream', StreamBackendRefsEditor],
] as const)('%s backend namespace clearing', (_name, Editor) => {
  it('restores omission without changing port, zero weight, filters or sibling references', () => {
    const onChange = vi.fn()
    const backend = {
      name: 'api', namespace: 'shared-services', group: '', kind: 'Service', port: 8443, weight: 0,
      filters: [{ type: 'RequestHeaderModifier' as const, requestHeaderModifier: { set: [{ name: 'x-tenant', value: 'edge' }] } }],
      futureRef: { retained: true },
    }
    const sibling = { name: 'fallback', namespace: 'fallback-services', port: 8080, weight: 20 }
    render(<Editor namespace="edge" value={[backend, sibling]} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue('shared-services'), { target: { value: '' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...backend, namespace: undefined }, sibling])
  })
})
