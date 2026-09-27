import { beforeEach, expect, it, vi } from 'vitest'
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { App } from 'antd'
import { I18nProvider } from '@/i18n'
import { renderWithQueryClient } from '@/test/render'
import type { RegionRouteOverrideListResult } from '@/api/regionRoute'
import RegionRouteOverridePage from './RegionRouteOverridePage'

const api = vi.hoisted(() => ({ listOverrides: vi.fn(), overrideFailover: vi.fn(), syncOverride: vi.fn() }))
vi.mock('@/api/regionRoute', () => ({ regionRouteApi: api }))
vi.mock('@/utils/permissions', () => ({ useCan: () => true }))

function snapshot(first = '', second = ''): RegionRouteOverrideListResult {
  return {
    success: true,
    onlineControllerIds: ['a', 'b'],
    data: [{
      namespace: 'shop', name: 'route',
      controllers: Object.fromEntries([first, second].map((failoverTo, index) => [
        index === 0 ? 'a' : 'b',
        {
          apiVersion: 'edgion.io/v1', kind: 'EdgionConfigData',
          metadata: { name: 'route', namespace: 'shop' },
          spec: { data: { type: 'RegionRouteOverride', config: { regions: [{ name: 'east', failoverTo }, { name: 'west' }] } } },
        },
      ])),
    }],
  }
}

beforeEach(() => {
  api.listOverrides.mockReset().mockResolvedValue(snapshot())
  api.overrideFailover.mockReset()
  api.syncOverride.mockReset()
})

it.each([
  { label: 'inconsistent', second: '', states: ['converged', 'conflict'], failed: 1 },
  { label: 'consistent but unconfirmed', second: 'west', states: ['accepted', 'unknown'], failed: 0 },
  { label: 'superseded', second: 'west', states: ['converged', 'superseded'], failed: 0 },
])('keeps outcomes visible after a $label watch refresh', async ({ second, states, failed }) => {
  api.overrideFailover.mockImplementation(async () => {
    api.listOverrides.mockResolvedValue(snapshot('west', second))
    return { modified: 2 - failed, failed, outcomes: [
      { controllerId: 'a', state: states[0] },
      { controllerId: 'b', state: states[1], observed: snapshot('west').data[0].controllers.a },
    ] }
  })
  renderWithQueryClient(<App><I18nProvider><RegionRouteOverridePage /></I18nProvider></App>)
  fireEvent.click(await screen.findByTestId('region-failover'))
  const select = await screen.findByTestId('region-failover-select-east')
  fireEvent.mouseDown(within(select).getByRole('combobox'))
  const option = await waitFor(() => {
    const element = document.querySelector('.ant-select-dropdown .ant-select-item-option[title="west"]')
    expect(element).not.toBeNull()
    return element!
  })
  fireEvent.click(option)
  fireEvent.click(screen.getByTestId('region-failover-apply'))
  await waitFor(() => expect(api.overrideFailover).toHaveBeenCalledWith('shop', 'route', 'east', 'west'))
  expect(await screen.findByTestId(`write-outcome-${states[1]}`)).toBeVisible()
  expect(screen.getByTestId(`write-outcome-${states[0]}`)).toBeVisible()
  if (failed) {
    expect(screen.getByTestId('region-failover')).toBeDisabled()
    expect(screen.getByTestId('region-failover-apply')).toBeDisabled()
  }
  expect(api.overrideFailover).toHaveBeenCalledTimes(1)
})


it.each(['accepted', 'unknown', 'superseded'])('retains sync %s after the row becomes consistent', async (state) => {
  api.listOverrides.mockResolvedValue(snapshot('west', ''))
  api.syncOverride.mockImplementation(async () => {
    api.listOverrides.mockResolvedValue(snapshot('west', 'west'))
    return { modified: 1, failed: 0, outcomes: [{ controllerId: 'b', state, observed: snapshot('west', 'west').data[0].controllers.b }] }
  })
  renderWithQueryClient(<App><I18nProvider><RegionRouteOverridePage /></I18nProvider></App>)
  fireEvent.click(await screen.findByTestId('region-sync-apply'))
  await waitFor(() => expect(api.syncOverride).toHaveBeenCalledWith('shop', 'route', 'a', ['b']))
  await waitFor(() => expect(screen.queryByTestId('region-sync-apply')).not.toBeInTheDocument())
  expect(screen.getByTestId(`write-outcome-${state}`)).toBeVisible()
  expect(screen.getByText('Consistent')).toBeVisible()
})
