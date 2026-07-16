import React from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import ExternalAuthFilterEditor, {
  parseJsonValueInput,
  switchSubjectAltNameType,
} from './ExternalAuthFilterEditor'
import type { HTTPExternalAuthFilter } from '@/types/gateway-api/httproute'

const initial: HTTPExternalAuthFilter = {
  target: { name: 'auth', port: 8080 },
  tls: {
    enabled: true,
    validation: {
      subjectAltNames: [{ type: 'URI', uri: 'spiffe://cluster/auth', futureSan: { keep: true } }],
    },
  },
  success: {
    body: [
      { pointer: '/number', equals: 1 },
      { pointer: '/bool', notEquals: true },
      { pointer: '/null', equals: null },
      { pointer: '/object', equals: { role: 'admin' } },
      { pointer: '/array', equals: [1, 'two'] },
      { pointer: '/mixed', in: [1, true, null, { nested: 'yes' }, ['array']] },
    ],
  },
  decision: {},
}

function Harness() {
  const [value, setValue] = React.useState(initial)
  return <>
    <ExternalAuthFilterEditor value={value} onChange={setValue} disabled={false} />
    <pre data-testid="wire">{JSON.stringify(value)}</pre>
  </>
}

describe('ExternalAuthFilterEditor wire values', () => {
  it('switches SAN type by removing only hostname/uri and preserving unknown fields', () => {
    expect(switchSubjectAltNameType({
      type: 'Hostname', hostname: 'auth.example.com', uri: 'legacy', futureSan: { keep: true },
    }, 'URI')).toEqual({ type: 'URI', uri: '', futureSan: { keep: true } })
  })

  it('round-trips URI SAN and every serde_json::Value predicate shape', () => {
    render(<Harness />)

    fireEvent.change(screen.getByLabelText('san-0-uri'), { target: { value: 'spiffe://cluster/new-auth' } })
    fireEvent.change(screen.getByLabelText('predicate-0-equals-json-value'), { target: { value: '2.5' } })
    fireEvent.change(screen.getByLabelText('predicate-1-notEquals-json-value'), { target: { value: 'false' } })
    fireEvent.change(screen.getByLabelText('predicate-2-equals-json-value'), { target: { value: 'null' } })
    fireEvent.change(screen.getByLabelText('predicate-3-equals-json-value'), { target: { value: '{"role":"operator"}' } })
    fireEvent.change(screen.getByLabelText('predicate-4-equals-json-value'), { target: { value: '[3,{"deep":true}]' } })
    fireEvent.change(screen.getByLabelText('predicate-5-in-json-value'), { target: { value: '[0,false,null,{"nested":"changed"},[1,2]]' } })

    const wire = JSON.parse(screen.getByTestId('wire').textContent || '{}')
    expect(wire.tls.validation.subjectAltNames[0]).toEqual({
      type: 'URI', uri: 'spiffe://cluster/new-auth', futureSan: { keep: true },
    })
    expect(wire.success.body.map((predicate: any) => predicate.equals ?? predicate.notEquals ?? predicate.in)).toEqual([
      2.5,
      false,
      undefined,
      { role: 'operator' },
      [3, { deep: true }],
      [0, false, null, { nested: 'changed' }, [1, 2]],
    ])
    expect(wire.success.body[2]).toEqual({ pointer: '/null', equals: null })
  })

  it('rejects invalid JSON values without mutating the wire value', () => {
    render(<Harness />)
    fireEvent.change(screen.getByLabelText('predicate-3-equals-json-value'), { target: { value: '{invalid' } })
    fireEvent.change(screen.getByLabelText('predicate-5-in-json-value'), { target: { value: '{"not":"an array"}' } })
    const wire = JSON.parse(screen.getByTestId('wire').textContent || '{}')
    expect(wire.success.body[3].equals).toEqual({ role: 'admin' })
    expect(wire.success.body[5].in).toEqual([1, true, null, { nested: 'yes' }, ['array']])
    expect(() => parseJsonValueInput('{invalid')).toThrow()
  })
})
