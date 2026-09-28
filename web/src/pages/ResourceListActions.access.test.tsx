import { cleanup, fireEvent, screen } from '@testing-library/react'
import { QueryClientProvider } from '@tanstack/react-query'
import { Modal } from 'antd'
import { afterEach, expect, it, vi } from 'vitest'
import { renderWithQueryClient } from '@/test/render'
import Page0 from './Infrastructure/GatewayList'
import Page1 from './Infrastructure/GatewayClassList'
import Page2 from './Infrastructure/ReferenceGrantList'
import Page3 from './Routes/HTTPRouteList'
import Page4 from './Routes/GRPCRouteList'
import Page5 from './Routes/TCPRouteList'
import Page6 from './Routes/UDPRouteList'
import Page7 from './Routes/TLSRouteList'
import Page8 from './Security/EdgionTlsList'
import Page9 from './System/EdgionAcmeList'
import Page10 from './System/EdgionGatewayConfigPage'

const state = vi.hoisted(() => ({ kind: '', verbs: ['get', 'list'], pending: false }))
const row = vi.hoisted(() => ({ apiVersion: 'v1', kind: 'Fixture', metadata: { name: 'fixture', namespace: 'app', resourceVersion: '1' }, spec: { listeners: [], rules: [], from: [], to: [], hosts: [], targetRefs: [] } }))
vi.mock('@/hooks/useResourceList', () => ({ useResourceList: () => ({ items: [row], isLoading: false, error: null, refetch: vi.fn(), hasNextPage: false }) }))
vi.mock('@/hooks/useControllerAccess', () => ({ useControllerAccess: () => ({ authorizationPending: state.pending, data: { resources: [{ kind: state.kind, verbs: state.verbs }], operations: [] } }) }))
vi.mock('@/hooks/useServerInfo', () => ({ useServerInfo: () => ({ data: { data: { capabilities: {} } } }) }))
vi.mock('@/utils/permissions', () => ({ usePermissions: () => ({ loading: false, permissions: ['proxy:read', 'proxy:write'] }) }))
vi.mock('react-router-dom', async () => ({ ...await vi.importActual('react-router-dom'), useParams: () => ({ controllerId: 'east~controller' }) }))
vi.mock('@/api/resources', () => ({ resourceApi: {}, clusterResourceApi: { listAll: async () => ({ data: [row] }) }, batchDeleteFailureKeys: () => null }))
vi.mock('@/components/resource/ResourceStatus', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/Gateway/GatewayEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/GatewayClass/GatewayClassEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/ReferenceGrant/ReferenceGrantEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/HTTPRoute/HTTPRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/GRPCRoute/GRPCRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/TCPRoute/TCPRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/UDPRoute/UDPRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/TLSRoute/TLSRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/EdgionTls/EdgionTlsEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/EdgionAcme/EdgionAcmeEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/EdgionGatewayConfig/EdgionGatewayConfigEditor', () => ({ default: () => null }))
afterEach(() => { cleanup(); vi.restoreAllMocks() })

const cases = [
  ['gateway', 'Gateway', Page0],
  ['gatewayclass', 'GatewayClass', Page1],
  ['referencegrant', 'ReferenceGrant', Page2],
  ['httproute', 'HTTPRoute', Page3],
  ['grpcroute', 'GRPCRoute', Page4],
  ['tcproute', 'TCPRoute', Page5],
  ['udproute', 'UDPRoute', Page6],
  ['tlsroute', 'TLSRoute', Page7],
  ['edgiontls', 'EdgionTls', Page8],
  ['edgionacme', 'EdgionAcme', Page9],
  ['edgiongatewayconfig', 'EdgionGatewayConfig', Page10],
 ] as const
it.each(cases)('%s actions follow read-only, pending, granted and revoked access', async (kind, canonical, Page) => {
  state.kind = canonical; state.verbs = ['get', 'list']; state.pending = false
  const confirm = vi.spyOn(Modal, 'confirm')
  const view = renderWithQueryClient(<Page />)
  await screen.findByTestId(`${kind}-row-view`)
  const rerender = () => view.rerender(<QueryClientProvider client={view.queryClient}><Page /></QueryClientProvider>)
  const writes = ['create', 'row-edit', 'row-delete']
  const checkbox = screen.queryAllByRole('checkbox')[1]
  if (checkbox) { fireEvent.click(checkbox); writes.push('batch-delete') }
  for (const action of writes) {
    const button = screen.getByTestId(`${kind}-${action}`)
    expect(button).toBeDisabled()
    fireEvent.click(button)
  }
  expect(confirm).not.toHaveBeenCalled()
  expect(screen.getByTestId(`${kind}-row-view`)).toBeEnabled()
  expect(screen.getByTestId(`${kind}-refresh`)).toBeEnabled()
  state.pending = true; rerender()
  expect(screen.getByTestId(`${kind}-row-view`)).toBeDisabled()
  expect(screen.getByTestId(`${kind}-refresh`)).toBeDisabled()
  state.pending = false; state.verbs = ['get', 'list', 'create', 'update']; rerender()
  expect(screen.getByTestId(`${kind}-create`)).toBeEnabled()
  expect(screen.getByTestId(`${kind}-row-edit`)).toBeEnabled()
  expect(screen.getByTestId(`${kind}-row-delete`)).toBeDisabled()
  state.verbs.push('delete'); rerender()
  for (const action of writes) expect(screen.getByTestId(`${kind}-${action}`)).toBeEnabled()
  state.verbs = ['get', 'list']; rerender()
  for (const action of writes) expect(screen.getByTestId(`${kind}-${action}`)).toBeDisabled()
  view.unmount(); view.queryClient.clear()
})
