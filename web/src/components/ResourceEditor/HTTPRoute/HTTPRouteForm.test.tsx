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

function Harness() {
  const [value, setValue] = useState(initial)
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
