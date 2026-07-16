import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { MemoryRouter } from 'react-router-dom'
import RegionRouteServiceUsagePage, { buildServiceManagementRows } from './RegionRouteServiceUsagePage'

const mocks = vi.hoisted(() => ({ list: vi.fn() }))
vi.mock('@/api/regionRoute', () => ({ regionRouteApi: { listRegionRoutes: mocks.list } }))

describe('RegionRouteServiceUsagePage', () => {
  const response = {
      success: true,
      data: [{
        namespace: 'shop', pluginName: 'region', alias: 'primary', entryIndex: 0,
        onlineControllerIds: ['east/controller-a', 'west/controller-b'], controllers: {
          'east/controller-a': { regions: [], serviceUsages: [{ routeKind: 'HTTPRoute', routeNamespace: 'shop', routeName: 'checkout', ruleIndex: 0, backendServices: [{ namespace: 'shop', name: 'checkout-api', port: 8080 }] }] },
          'west/controller-b': { regions: [], serviceUsages: [{ routeKind: 'HTTPRoute', routeNamespace: 'shop', routeName: 'checkout', ruleIndex: 0, backendServices: [{ namespace: 'shop', name: 'checkout-api', port: 8080 }] }] },
        },
      }],
    }

  it('aggregates the same service usage across controllers into one management row', () => {
    const rows = buildServiceManagementRows(response.data as never)
    expect(rows).toHaveLength(1)
    expect(Object.keys(rows[0].controllers)).toEqual(['east/controller-a', 'west/controller-b'])
    expect(rows[0].issues).toEqual([])
  })

  it('reports missing usage and backend differences across controllers', () => {
    const routes = structuredClone(response.data)
    routes[0].controllers['west/controller-b'].serviceUsages = []
    let rows = buildServiceManagementRows(routes as never)
    expect(rows[0].issues).toContain('missingUsage:west/controller-b')

    routes[0].controllers['west/controller-b'].serviceUsages = [{
      routeKind: 'HTTPRoute', routeNamespace: 'shop', routeName: 'checkout', ruleIndex: 0,
      backendServices: [{ namespace: 'shop', name: 'checkout-canary', port: 8080 }],
    }]
    rows = buildServiceManagementRows(routes as never)
    expect(rows[0].issues).toContain('backendMismatch')
  })

  it('counts an online controller that does not report the RegionRoute as missing', () => {
    const routes = structuredClone(response.data)
    routes[0].onlineControllerIds.push('north/controller-c')
    const rows = buildServiceManagementRows(routes as never)
    expect(Object.keys(rows[0].controllers)).toEqual(['east/controller-a', 'west/controller-b', 'north/controller-c'])
    expect(rows[0].issues).toContain('missingUsage:north/controller-c')
  })

  it('marks coverage unknown when an older backend omits fleet membership', () => {
    const routes = structuredClone(response.data)
    delete (routes[0] as { onlineControllerIds?: string[] }).onlineControllerIds
    const rows = buildServiceManagementRows(routes as never)
    expect(rows[0].membershipKnown).toBe(false)
    expect(rows[0].issues).toContain('membershipUnknown')
  })

  it('renders the service-oriented management view', async () => {
    mocks.list.mockResolvedValue(response)
    render(<MemoryRouter><QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}><RegionRouteServiceUsagePage /></QueryClientProvider></MemoryRouter>)
    expect(await screen.findByText('shop/checkout-api:8080')).toBeInTheDocument()
    expect(screen.getByText('2/2')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Manage Region/i })).toBeInTheDocument()
  })
})
