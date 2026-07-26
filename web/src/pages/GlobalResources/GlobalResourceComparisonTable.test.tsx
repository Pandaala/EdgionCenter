import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { GlobalResourceComparisonGroup } from '@/api/globalResources'
import GlobalResourceComparisonTable from './GlobalResourceComparisonTable'

vi.mock('@/i18n', () => ({
  useT: () => (key: string) => key,
}))

function member(cluster: string, object: GlobalResourceComparisonGroup['members'][number]['object']) {
  return { cluster, controllerId: `controller-${cluster}`, object }
}

const groups: GlobalResourceComparisonGroup[] = [
  {
    key: { kind: 'HTTPRoute', namespace: 'edgion-system', name: 'single-route' },
    members: [member('alpha', { spec: { hostnames: ['single.example.com'] } })],
  },
  {
    key: { kind: 'HTTPRoute', namespace: 'edgion-system', name: 'consistent-route' },
    members: [
      member('alpha', {
        metadata: { resourceVersion: '1' },
        spec: { hostnames: ['same.example.com'] },
      }),
      member('beta', {
        spec: { hostnames: ['same.example.com'] },
        metadata: { resourceVersion: '2' },
      }),
    ],
  },
  {
    key: { kind: 'HTTPRoute', namespace: 'edgion-system', name: 'inconsistent-route' },
    members: [
      member('alpha', { spec: { hostnames: ['a.example.com'] } }),
      member('beta', { spec: { hostnames: ['b.example.com'] } }),
    ],
  },
]

describe('GlobalResourceComparisonTable', () => {
  it('renders single, consistent, and inconsistent comparison states', () => {
    const onOpen = vi.fn()
    render(<GlobalResourceComparisonTable groups={groups} onOpen={onOpen} />)

    expect(screen.getByTestId('global-resource-consistency-single')).toHaveTextContent(
      'globalResources.consistency.single',
    )
    expect(screen.getByTestId('global-resource-consistency-consistent')).toHaveTextContent(
      'globalResources.consistency.consistent',
    )
    expect(screen.getByTestId('global-resource-consistency-inconsistent')).toHaveTextContent(
      'globalResources.consistency.inconsistent',
    )

    fireEvent.click(screen.getAllByText('globalResources.action.compare')[2])
    expect(onOpen).toHaveBeenCalledWith(groups[2])
  })
})
