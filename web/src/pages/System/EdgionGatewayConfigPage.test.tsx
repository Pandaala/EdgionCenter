import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { clusterResourceApi } from '@/api/resources'
import { renderWithQueryClient } from '@/test/render'
import EdgionGatewayConfigPage from './EdgionGatewayConfigPage'

vi.mock('@/components/ResourceEditor/EdgionGatewayConfig/EdgionGatewayConfigEditor', () => ({ default: () => null }))
vi.mock('@/components/resource/ResourceStatus', () => ({ default: () => null }))
vi.mock('react-router-dom', async () => ({ ...await vi.importActual('react-router-dom'), useParams: () => ({ controllerId: 'east~controller' }) }))
afterEach(() => { cleanup(); vi.restoreAllMocks() })
const item = (name: string, attempts: number) => ({ apiVersion: 'edgion.io/v1alpha1', kind: 'EdgionGatewayConfig', metadata: { name }, spec: { maxRetries: 999, retry: { attempts } } })

it('shows current retry attempts, preserves explicit zero and ignores the removed root field', async () => {
  vi.spyOn(clusterResourceApi, 'listAll').mockResolvedValue({ success: true, data: [item('no-retry', 0), item('retry-three', 3)], count: 2 })
  renderWithQueryClient(<EdgionGatewayConfigPage />)
  const zero = (await screen.findByText('no-retry')).closest('tr')!
  const three = (await screen.findByText('retry-three')).closest('tr')!
  expect(within(zero).getByText('0')).toBeInTheDocument()
  expect(within(three).getByText('3')).toBeInTheDocument()
  expect(screen.queryByText('999')).not.toBeInTheDocument()
})

it('shows a list failure instead of an empty table and retries to recovery', async () => {
  const list = vi.spyOn(clusterResourceApi, 'listAll').mockRejectedValueOnce(new Error('List unavailable'))
    .mockResolvedValue({ success: true, data: [item('recovered', 2)], count: 1 })
  renderWithQueryClient(<EdgionGatewayConfigPage />)
  fireEvent.click(await screen.findByTestId('resource-list-retry'))
  expect(await screen.findByText('recovered')).toBeInTheDocument()
  expect(list).toHaveBeenCalledTimes(2)
})
