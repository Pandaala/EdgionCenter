import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { GlobalResourceComparisonGroup } from '@/api/globalResources'
import GlobalResourceComparisonTable from './GlobalResourceComparisonTable'

vi.mock('@/i18n', () => ({
  useT: () => (key: string) => key,
}))

function member(
  cluster: string,
  object: GlobalResourceComparisonGroup['members'][number]['object'],
  syncState?: GlobalResourceComparisonGroup['members'][number]['syncState'],
) {
  return { cluster, controllerId: `controller-${cluster}`, object, syncState }
}

const groups: GlobalResourceComparisonGroup[] = [
  {
    key: { kind: 'EdgionConfigData', namespace: 'edgion-system', name: 'single-route' },
    members: [member('alpha', { spec: { hostnames: ['single.example.com'] } })],
  },
  {
    key: { kind: 'EdgionConfigData', namespace: 'edgion-system', name: 'consistent-route' },
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
    key: { kind: 'EdgionConfigData', namespace: 'edgion-system', name: 'inconsistent-route' },
    members: [
      member('alpha', { spec: { hostnames: ['a.example.com'] } }),
      member('beta', { spec: { hostnames: ['b.example.com'] } }),
    ],
  },
]

const syncGroups: GlobalResourceComparisonGroup[] = [
  {
    key: { kind: 'EdgionConfigData', namespace: 'edgion-system', name: 'mixed-sync-route' },
    members: [
      member('alpha', { spec: { hostnames: ['ok.example.com'] } }, 'ok'),
      member('beta', { spec: { hostnames: ['ok.example.com'] } }, 'stale'),
    ],
  },
  {
    key: { kind: 'EdgionConfigData', namespace: 'edgion-system', name: 'all-ok-route' },
    members: [
      member('alpha', { spec: { hostnames: ['ok.example.com'] } }, 'ok'),
      member('beta', { spec: { hostnames: ['ok.example.com'] } }, 'ok'),
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

  it('renders the group-level worst sync state across members', () => {
    render(<GlobalResourceComparisonTable groups={syncGroups} onOpen={vi.fn()} />)

    const mixedRow = screen.getByText('mixed-sync-route').closest('tr')
    expect(mixedRow).not.toBeNull()
    expect(within(mixedRow!).getByTestId('global-resource-sync-stale')).toHaveTextContent(
      'globalResources.syncState.stale',
    )

    const allOkRow = screen.getByText('all-ok-route').closest('tr')
    expect(allOkRow).not.toBeNull()
    expect(within(allOkRow!).getByTestId('global-resource-sync-ok')).toHaveTextContent(
      'globalResources.syncState.ok',
    )
  })
})
