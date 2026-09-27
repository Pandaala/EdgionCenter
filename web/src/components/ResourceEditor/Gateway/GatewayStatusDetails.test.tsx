import { render, screen } from '@testing-library/react'
import { expect, it } from 'vitest'
import GatewayStatusDetails from './GatewayStatusDetails'

it('renders native Gateway addresses and listener counters without duplicating conditions', () => {
  render(<GatewayStatusDetails generation={4} status={{
    addresses: [{ type: 'IPAddress', value: '192.0.2.10' }],
    conditions: [{ type: 'ListenersNotValid', status: 'True', observedGeneration: 4 }],
    listeners: [{
      name: 'https', attachedRoutes: 2,
      supportedKinds: [{ group: 'gateway.networking.k8s.io', kind: 'HTTPRoute' }],
      conditions: [{ type: 'Conflicted', status: 'True', observedGeneration: 4 }],
    }, {
      name: 'http', attachedRoutes: 0, supportedKinds: [],
      conditions: [{ type: 'Programmed', status: 'True', observedGeneration: 3 }],
    }],
  }} />)
  expect(screen.getByText('IPAddress: 192.0.2.10')).toBeVisible()
  expect(screen.getByText('https')).toBeVisible()
  expect(screen.getByText('http')).toBeVisible()
  expect(screen.getByText('2')).toBeVisible()
  expect(screen.getByText('0')).toBeVisible()
  expect(screen.getByText('gateway.networking.k8s.io/HTTPRoute')).toBeVisible()
  expect(screen.getByText('ListenersNotValid=True')).toHaveClass('ant-tag-red')
  expect(screen.getAllByText('Conflicted=True')).toHaveLength(1)
  expect(screen.getByText('Programmed=True (stale)')).toHaveClass('ant-tag-gold')
})

it('renders an empty Gateway status without inventing listeners or addresses', () => {
  render(<GatewayStatusDetails status={undefined} />)
  expect(screen.getByText('No status conditions reported')).toBeVisible()
  expect(screen.queryByText('0')).not.toBeInTheDocument()
})
