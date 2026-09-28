import { act, cleanup, fireEvent, screen, waitFor } from '@testing-library/react'
import { Modal, message } from 'antd'
import { afterEach, expect, it, vi } from 'vitest'
import { renderWithQueryClient } from '@/test/render'
import Page0 from './Routes/HTTPRouteList'
import Page1 from './Routes/GRPCRouteList'
import Page2 from './Routes/TCPRouteList'
import Page3 from './Routes/UDPRouteList'
import Page4 from './Routes/TLSRouteList'
import Page5 from './Infrastructure/GatewayList'
import Page6 from './Security/EdgionTlsList'
import Page7 from './Plugins/EdgionPluginsList'
import Page8 from './Plugins/EdgionStreamPluginsList'
import Page9 from './Infrastructure/ServiceList'
import Page10 from './Infrastructure/EndpointSliceList'
import Page11 from './Security/BackendTLSPolicyList'
import Page12 from './System/LinkSysList'
const mocks = vi.hoisted(() => ({ batchDelete: vi.fn() }))
vi.mock('@/api/resources', () => ({ resourceApi: { batchDelete: (...args: unknown[]) => mocks.batchDelete(...args) }, batchDeleteFailureKeys: () => null }))
vi.mock('@/hooks/useResourceList', () => ({ useResourceList: (kind: string) => ({ items: ['a', 'b', 'c'].map((suffix) => ({ apiVersion: 'v1', kind, metadata: { name: `${kind}-${suffix}`, namespace: 'app', resourceVersion: suffix }, spec: { rules: [], listeners: [], hosts: [], targetRefs: [], type: 'redis', config: {}, requestPlugins: [] } })), isLoading: false, error: null, refetch: vi.fn(), hasNextPage: false }) }))
vi.mock('@/hooks/useControllerAccess', () => ({ useControllerAccess: () => ({ canResource: () => true }) }))
vi.mock('@/components/resource/PermissionAwareButton', () => ({ default: (props: any) => <button data-testid={props['data-testid']} onClick={props.onClick}>{props.children}</button> }))
vi.mock('@/components/resource/ResourceStatus', () => ({ default: () => null }))
vi.mock('react-router-dom', async () => ({ ...await vi.importActual('react-router-dom'), useParams: () => ({ controllerId: 'east~controller' }) }))
vi.mock('@/components/ResourceEditor/HTTPRoute/HTTPRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/GRPCRoute/GRPCRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/TCPRoute/TCPRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/UDPRoute/UDPRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/TLSRoute/TLSRouteEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/Gateway/GatewayEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/EdgionTls/EdgionTlsEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/EdgionPlugins/EdgionPluginsEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/EdgionStreamPlugins/EdgionStreamPluginsEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/Service/ServiceEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/EndpointSlice/EndpointSliceEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/BackendTLSPolicy/BackendTLSPolicyEditor', () => ({ default: () => null }))
vi.mock('@/components/ResourceEditor/LinkSys/LinkSysEditor', () => ({ default: () => null }))
afterEach(() => { cleanup(); vi.restoreAllMocks(); mocks.batchDelete.mockReset() })
const cases = [
  ['httproute', Page0],
  ['grpcroute', Page1],
  ['tcproute', Page2],
  ['udproute', Page3],
  ['tlsroute', Page4],
  ['gateway', Page5],
  ['edgiontls', Page6],
  ['edgionplugins', Page7],
  ['edgionstreamplugins', Page8],
  ['service', Page9],
  ['endpointslice', Page10],
  ['backendtlspolicy', Page11],
  ['linksys', Page12],
 ] as const
it.each(cases)('%s retains filtered selections and reports the submitted count', async (kind, Page) => {
  let finish!: () => void
  mocks.batchDelete.mockImplementation(() => new Promise<void>((resolve) => { finish = resolve }))
  const confirm = vi.spyOn(Modal, 'confirm').mockReturnValue({ destroy: vi.fn(), update: vi.fn() })
  const success = vi.spyOn(message, 'success').mockImplementation(() => undefined as any)
  const view = renderWithQueryClient(<Page />)
  const boxes = screen.getAllByRole('checkbox')
  fireEvent.click(boxes[1]); fireEvent.click(boxes[2])
  fireEvent.change(screen.getByTestId(`${kind}-search`), { target: { value: `${kind}-a` } })
  expect(screen.queryByText(`${kind}-b`, { exact: true })).not.toBeInTheDocument()
  fireEvent.click(screen.getByTestId(`${kind}-batch-delete`))
  const dialog = confirm.mock.calls[0][0]
  expect(dialog.content).toContain('2')
  await act(async () => { dialog.onOk?.() })
  await waitFor(() => expect(mocks.batchDelete).toHaveBeenCalledWith({ controllerId: 'east/controller' }, kind, [
    { namespace: 'app', name: `${kind}-a`, resourceVersion: 'a' },
    { namespace: 'app', name: `${kind}-b`, resourceVersion: 'b' },
  ]))
  // Changing selection while the request is in flight must not alter its outcome count.
  fireEvent.click(screen.getAllByRole('checkbox')[1])
  await act(async () => { finish() })
  await waitFor(() => expect(success).toHaveBeenCalledWith(expect.stringContaining('2')))
  view.unmount(); view.queryClient.clear()
})
