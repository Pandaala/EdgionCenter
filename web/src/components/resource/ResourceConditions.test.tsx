import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import ResourceConditions, { collectResourceConditions } from './ResourceConditions'

describe('ResourceConditions', () => {
  const status = {
    conditions: [{ type: 'Accepted', status: 'True', reason: 'Accepted' }],
    parents: [{
      parentRef: { namespace: 'edge', name: 'gateway', sectionName: 'https' },
      conditions: [{ type: 'ResolvedRefs', status: 'False', reason: 'BackendNotFound', message: 'Missing Service' }],
    }],
    ancestors: [{
      ancestorRef: { name: 'service' },
      conditions: [{ type: 'Conflicted', status: 'True', reason: 'LostOldestWins' }],
    }],
    listeners: [{
      name: 'web',
      conditions: [{ type: 'Programmed', status: 'Unknown', reason: 'Pending' }],
    }],
  }

  it('collects direct, parent, ancestor, and listener conditions with context', () => {
    expect(collectResourceConditions(status).map((item) => item.context)).toEqual([
      'Resource',
      'Parent: edge/gateway#https',
      'Ancestor: service',
      'Listener: web',
    ])
  })

  it('renders compact status without inventing an Active state', () => {
    render(<ResourceConditions status={status} compact />)

    expect(screen.getByText('Accepted=True')).toBeInTheDocument()
    expect(screen.getByText('ResolvedRefs=False')).toBeInTheDocument()
    expect(screen.queryByText('Active')).not.toBeInTheDocument()
  })

  it('distinguishes permitted and denied cross-namespace references', () => {
    const { rerender } = render(<ResourceConditions compact status={{
      parents: [{ conditions: [{ type: 'ResolvedRefs', status: 'False', reason: 'RefNotPermitted' }] }],
    }} />)
    expect(screen.getByTestId('route-ref-denied')).toHaveTextContent('ResolvedRefs=False')
    expect(screen.queryByTestId('route-ref-granted')).not.toBeInTheDocument()

    rerender(<ResourceConditions compact status={{
      parents: [{ conditions: [{ type: 'ResolvedRefs', status: 'True', reason: 'ResolvedRefs' }] }],
    }} />)
    expect(screen.getByTestId('route-ref-granted')).toHaveTextContent('ResolvedRefs=True')
    expect(screen.queryByTestId('route-ref-denied')).not.toBeInTheDocument()
  })
})
