import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import ResourceConditions, { collectResourceConditions } from './ResourceConditions'

describe('ResourceConditions', () => {
  it('preserves opposing deployment observations in the Kubernetes envelope', () => {
    const accepted = { type: 'Accepted', status: 'True', observedGeneration: 7 }
    const rejected = { type: 'Accepted', status: 'False', reason: 'Invalid', observedGeneration: 6 }
    const partitioned = { controllers: [
      { controllerName: 'edgion.io/east', status: { conditions: [accepted] } },
      { controllerName: 'edgion.io/west', status: { conditions: [rejected] } },
    ] }
    expect(collectResourceConditions(partitioned)).toEqual([
      { context: 'Controller: edgion.io/east', condition: accepted },
      { context: 'Controller: edgion.io/west', condition: rejected },
    ])
    render(<ResourceConditions status={partitioned} />)
    expect(screen.getByText('Controller: edgion.io/east')).toBeInTheDocument()
    expect(screen.getByText('Controller: edgion.io/west')).toBeInTheDocument()
    expect(screen.getByText('Accepted=True')).toBeInTheDocument()
    expect(screen.getByText('Accepted=False')).toBeInTheDocument()
  })

  it('ignores malformed observations without losing valid neighbors or using top-level fallback', () => {
    const condition = { type: 'ResolvedRefs', status: 'False' }
    expect(collectResourceConditions({
      conditions: [{ type: 'Accepted', status: 'True' }],
      controllers: [null, {}, { controllerName: '' }, { controllerName: 'bad', status: null },
        { controllerName: 'valid', status: { conditions: [null, {}, condition] } }],
    })).toEqual([{ context: 'Controller: valid', condition }])
    expect(collectResourceConditions({ controllers: null, conditions: [condition] })).toEqual([])
  })

  it('identifies native parent and ancestor writers independently of attachment identity', () => {
    const condition = { type: 'Accepted', status: 'True' }
    expect(collectResourceConditions({
      parents: [{ controllerName: 'east', parentRef: { name: 'shared' }, conditions: [condition] }],
      ancestors: [{ controllerName: 'west', ancestorRef: { name: 'shared' }, conditions: [condition] }],
    }).map((item) => item.context)).toEqual([
      'Controller: east / Parent: shared', 'Controller: west / Ancestor: shared',
    ])
  })

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
