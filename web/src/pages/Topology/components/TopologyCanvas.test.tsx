import { render, screen, within } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'
import TopologyCanvas from './TopologyCanvas'
import { buildTopologyGraph } from '../hooks/useTopologyData'

afterEach(() => { vi.unstubAllGlobals() })

it('shows partial invalidity, stale observations and current rejection together', () => {
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
  const graph = buildTopologyGraph({ httproute: [{
    apiVersion: 'gateway.networking.k8s.io/v1', kind: 'HTTPRoute',
    metadata: { name: 'web', namespace: 'demo', generation: 3 },
    status: { conditions: [
      { type: 'Conflicted', status: 'True', observedGeneration: 2 },
      { type: 'Accepted', status: 'False', observedGeneration: 3 },
      { type: 'PartiallyInvalid', status: 'True', observedGeneration: 3 },
    ] },
  }] }, null, new Set(), true)
  render(<TopologyCanvas nodes={graph.nodes} edges={graph.edges} onNodeClick={vi.fn()} />)
  const node = within(screen.getByTestId('topology-node'))
  expect(node.getByText('stale status')).toHaveClass('ant-tag-gold')
  expect(node.getByText('partially invalid')).toHaveClass('ant-tag-orange')
  expect(node.getByText('rejected')).toHaveClass('ant-tag-red')
  expect(node.queryByText('conflict')).not.toBeInTheDocument()
})
