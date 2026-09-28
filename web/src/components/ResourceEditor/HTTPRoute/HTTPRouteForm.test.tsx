import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { describe, expect, it } from 'vitest'
import type { HTTPRoute } from '@/types/gateway-api'
import HTTPRouteForm from './HTTPRouteForm'

const initial: HTTPRoute = {
  apiVersion: 'gateway.networking.k8s.io/v1',
  kind: 'HTTPRoute',
  metadata: {
    name: 'route',
    namespace: 'edge',
    annotations: {
      'future.example.com/retained': 'yes',
      'edgion.io/mirror-log': 'false',
    },
  },
  spec: {
    parentRefs: [{ name: 'gateway' }],
    rules: [],
  },
}

function Harness({ resource = initial }: { resource?: HTTPRoute }) {
  const [value, setValue] = useState(resource)
  return <>
    <HTTPRouteForm value={value} onChange={setValue} isCreate={false} />
    <pre data-testid="wire">{JSON.stringify(value)}</pre>
  </>
}

describe('HTTPRouteForm mirror tuning annotations', () => {
  it('states route-wide scope and narrowly patches the selected annotation', () => {
    render(<Harness />)

    expect(screen.getByText(
      'These route annotations apply to every RequestMirror filter on this HTTPRoute.',
    )).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Connect Timeout (ms)'), { target: { value: '1500' } })

    const wire = JSON.parse(screen.getByTestId('wire').textContent || '{}')
    expect(wire.metadata.annotations).toEqual({
      'future.example.com/retained': 'yes',
      'edgion.io/mirror-log': 'false',
      'edgion.io/mirror-connect-timeout-ms': '1500',
    })
  })
})

it('clears optional parent namespace and listener without losing other references or fields', () => {
  const resource = {
    ...initial,
    spec: {
      ...initial.spec,
      parentRefs: [
        { name: 'gateway', namespace: 'shared-edge', sectionName: 'https', port: 443, group: 'gateway.networking.k8s.io', kind: 'Gateway', futureRef: true },
        { name: 'gateway-two', port: 8443 },
      ],
    },
  }
  render(<Harness resource={resource} />)
  expect(screen.getByText('Gateway Refs', { exact: true })).toBeInTheDocument()
  fireEvent.change(screen.getByDisplayValue('shared-edge'), { target: { value: '' } })
  fireEvent.change(screen.getByDisplayValue('https'), { target: { value: '' } })
  const wire = JSON.parse(screen.getByTestId('wire').textContent!)
  expect(wire.spec.parentRefs).toEqual([
    { name: 'gateway', port: 443, group: 'gateway.networking.k8s.io', kind: 'Gateway', futureRef: true },
    resource.spec.parentRefs[1],
  ])
  expect(wire.metadata).toEqual(resource.metadata)
  expect(wire.spec.rules).toEqual(resource.spec.rules)
})
