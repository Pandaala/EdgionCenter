import { useState, useEffect } from 'react'
import { Routes, Route, Navigate, useLocation } from 'react-router-dom'
import { Button, Result, Spin } from 'antd'
import { AppShell } from './components/shell/AppShell'
import ControllerProxy from './components/Layout/ControllerProxy'
import { isLoggedIn } from './utils/auth'
import { PermissionGate, PermissionProvider } from './utils/permissions'
import { setAppMode } from './utils/proxy'
import { resolveServerDiscovery } from './utils/discovery'
import { systemApi, type CenterCapabilities } from './api/client'
import LoginPage from './pages/Login/LoginPage'
import Dashboard from './pages/Dashboard'
import UserDashboard from './pages/Dashboard/UserDashboard'
import CenterDashboard from './pages/Center/CenterDashboard'
import ControllersPage from './pages/Center/ControllersPage'
import AuditLogPage from './pages/Audit/AuditLogPage'
import UserManagementPage from './pages/Users/UserManagementPage'
import RoleManagementPage from './pages/Roles/RoleManagementPage'
// RegionRoute
import RegionRouteList from './pages/RegionRoute/RegionRouteList'
import RegionRouteOverridePage from './pages/RegionRoute/RegionRouteOverridePage'
// Routes
import HTTPRouteList from './pages/Routes/HTTPRouteList'
import GRPCRouteList from './pages/Routes/GRPCRouteList'
import TCPRouteList from './pages/Routes/TCPRouteList'
import UDPRouteList from './pages/Routes/UDPRouteList'
import TLSRouteList from './pages/Routes/TLSRouteList'
// Infrastructure
import GatewayList from './pages/Infrastructure/GatewayList'
import GatewayClassList from './pages/Infrastructure/GatewayClassList'
import ServiceList from './pages/Infrastructure/ServiceList'
import EndpointSliceList from './pages/Infrastructure/EndpointSliceList'
import EdgionBackendList from './pages/Services/EdgionBackendList'
import EdgionBackendTrafficPolicyList from './pages/Services/EdgionBackendTrafficPolicyList'
import ReferenceGrantList from './pages/Infrastructure/ReferenceGrantList'
// Security
import EdgionTlsList from './pages/Security/EdgionTlsList'
import BackendTLSPolicyList from './pages/Security/BackendTLSPolicyList'
import RestrictedDependenciesPage from './pages/Security/RestrictedDependenciesPage'
// Plugins
import EdgionPluginsList from './pages/Plugins/EdgionPluginsList'
import EdgionStreamPluginsList from './pages/Plugins/EdgionStreamPluginsList'
import EdgionConfigDataList from './pages/Plugins/EdgionConfigDataList'
// System
import EdgionGatewayConfigPage from './pages/System/EdgionGatewayConfigPage'
import LinkSysList from './pages/System/LinkSysList'
import EdgionAcmeList from './pages/System/EdgionAcmeList'
import TopologyPage from './pages/Topology/TopologyPage'
import GlobalResourceInventoryPage from './pages/GlobalResources/GlobalResourceInventoryPage'
import { GLOBAL_RESOURCE_DESCRIPTORS } from './pages/GlobalResources/globalResourceDescriptors'
import ProviderAccountsPage from './pages/Cloud/ProviderAccountsPage'
import CloudflareDnsPage from './pages/Cloud/CloudflareDnsPage'
import Route53DnsPage from './pages/Cloud/Route53DnsPage'
import './App.css'

function RequireAuth({ children }: { children: React.ReactNode }) {
  const location = useLocation()
  if (!isLoggedIn()) {
    return <Navigate to="/login" replace state={{ from: `${location.pathname}${location.search}${location.hash}` }} />
  }
  // Fetch /auth/me once so menus and direct routes share one permission set.
  return <PermissionProvider>{children}</PermissionProvider>
}

function RequirePermission({ permission, children }: { permission: string; children: React.ReactNode }) {
  return (
    <PermissionGate
      permission={permission}
      pending={<Spin size="large" />}
      denied={<Navigate to="/" replace />}
    >
      {children}
    </PermissionGate>
  )
}

function RequirePermissions({ permissions, children }: { permissions: string[]; children: React.ReactNode }) {
  return permissions.reduceRight<React.ReactNode>(
    (content, permission) => <RequirePermission permission={permission}>{content}</RequirePermission>,
    children,
  )
}

function App() {
  const [mode, setMode] = useState<'controller' | 'center' | null>(null)
  const [capabilities, setCapabilities] = useState<CenterCapabilities | null>(null)
  const [loading, setLoading] = useState(true)
  const [discoveryFailed, setDiscoveryFailed] = useState(false)

  useEffect(() => {
    // server-info is unauthenticated — always call it to detect mode
    systemApi
      .serverInfo()
      .then((res) => {
        const discovery = resolveServerDiscovery(res)
        setMode(discovery.mode)
        setCapabilities(discovery.capabilities)
        setAppMode(discovery.mode)
      })
      .catch(() => {
        setDiscoveryFailed(true)
      })
      .finally(() => setLoading(false))
  }, [])

  if (loading) {
    return (
      <Spin
        size="large"
        style={{
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'center',
          minHeight: '100vh',
        }}
      />
    )
  }

  if (discoveryFailed || mode == null) {
    return (
      <Result
        status="warning"
        title="Unable to discover server capabilities"
        subTitle="The dashboard will not guess an authentication mode. Check the service and retry."
        extra={<Button type="primary" onClick={() => window.location.reload()}>Retry</Button>}
      />
    )
  }

  if (mode === 'center') {
    return (
      <Routes>
        <Route path="/login" element={<LoginPage passwordLogin={capabilities?.passwordLogin === true} />} />
        <Route path="/" element={<RequireAuth><AppShell mode="center" /></RequireAuth>}>
          <Route index element={<CenterDashboard />} />
          <Route path="controllers" element={<RequirePermission permission="controllers:read"><ControllersPage /></RequirePermission>} />
          <Route path="region-routes" element={<Navigate to="/region-routes/region" replace />} />
          <Route path="region-routes/region" element={<RequirePermission permission="region-routes:read"><RegionRouteOverridePage /></RequirePermission>} />
          <Route path="region-routes/topology" element={<Navigate to="/region-routes/region" replace />} />
          <Route path="region-routes/cluster" element={<Navigate to="/region-routes/region" replace />} />
          <Route path="region-routes/service" element={<Navigate to="/region-routes/region" replace />} />
          <Route path="region-routes/services" element={<Navigate to="/region-routes/region" replace />} />
          {capabilities?.providerAccountAdmin && <Route path="cloud/provider-accounts" element={<RequirePermission permission="provider-accounts:read"><ProviderAccountsPage /></RequirePermission>} />}
          {capabilities?.cloudflareDnsRead && <Route path="cloud/cloudflare/dns" element={<RequirePermissions permissions={['cloudflare-dns:read', 'provider-accounts:read']}><CloudflareDnsPage /></RequirePermissions>} />}
          {capabilities?.route53DnsRead && <Route path="cloud/aws/route53" element={<RequirePermissions permissions={['route53-dns:read', 'provider-accounts:read']}><Route53DnsPage dnsWriteAvailable={capabilities.route53DnsWrite === true} zoneLifecycleAvailable={capabilities.route53ZoneLifecycle === true} /></RequirePermissions>} />}
          {capabilities?.globalResourcesInventory && (
            <>
              <Route
                path="global-resources"
                element={<Navigate to={GLOBAL_RESOURCE_DESCRIPTORS[0].route} replace />}
              />
              <Route
                path="global-resources/edgion-config-data"
                element={<Navigate to={GLOBAL_RESOURCE_DESCRIPTORS[0].route} replace />}
              />
              {GLOBAL_RESOURCE_DESCRIPTORS.map((descriptor) => (
                <Route
                  key={descriptor.key}
                  path={descriptor.route.slice(1)}
                  element={(
                    <RequirePermission permission="global-resources:read">
                      <GlobalResourceInventoryPage descriptor={descriptor} />
                    </RequirePermission>
                  )}
                />
              ))}
              <Route
                path="global-resources/edgion-config-data/*"
                element={<Navigate to={GLOBAL_RESOURCE_DESCRIPTORS[0].route} replace />}
              />
              <Route
                path="global-resources/:legacyKind"
                element={<Navigate to={GLOBAL_RESOURCE_DESCRIPTORS[0].route} replace />}
              />
            </>
          )}
          <Route path="admin" element={<Navigate to="/controllers" replace />} />
          {capabilities?.auditQuery && <Route path="audit" element={<RequirePermission permission="audit:read"><AuditLogPage /></RequirePermission>} />}
          {capabilities?.userAdmin && <Route path="users" element={<RequirePermission permission="users:manage"><UserManagementPage /></RequirePermission>} />}
          {capabilities?.roleAdmin && <Route path="roles" element={<RequirePermission permission="roles:manage"><RoleManagementPage /></RequirePermission>} />}
        </Route>
        <Route path="/controller/:controllerId" element={<RequireAuth><ControllerProxy /></RequireAuth>}>
          <Route index element={<Dashboard />} />
          <Route path="user" element={<UserDashboard />} />
          <Route path="topology" element={<TopologyPage />} />
          <Route path="routes/http" element={<HTTPRouteList />} />
          <Route path="routes/grpc" element={<GRPCRouteList />} />
          <Route path="routes/tcp" element={<TCPRouteList />} />
          <Route path="routes/udp" element={<UDPRouteList />} />
          <Route path="routes/tls" element={<TLSRouteList />} />
          <Route path="infrastructure/gateways" element={<GatewayList />} />
          <Route path="infrastructure/gatewayclasses" element={<GatewayClassList />} />
          <Route path="infrastructure/referencegrants" element={<ReferenceGrantList />} />
          <Route path="services/list" element={<ServiceList />} />
          <Route path="services/endpointslices" element={<EndpointSliceList />} />
          <Route path="services/ai-backends" element={<EdgionBackendList />} />
          <Route path="services/backend-traffic-policies" element={<EdgionBackendTrafficPolicyList />} />
          <Route path="security/tls" element={<EdgionTlsList />} />
          <Route path="security/backendtls" element={<BackendTLSPolicyList />} />
          <Route path="security/dependencies" element={<RestrictedDependenciesPage />} />
          <Route path="plugins" element={<EdgionPluginsList />} />
          <Route path="plugins/stream" element={<EdgionStreamPluginsList />} />
          <Route path="plugins/metadata" element={<EdgionConfigDataList />} />
          <Route path="system/config" element={<EdgionGatewayConfigPage />} />
          <Route path="system/linksys" element={<LinkSysList />} />
          <Route path="system/acme" element={<EdgionAcmeList />} />
          <Route path="region-routes" element={<RegionRouteList />} />
        </Route>
      </Routes>
    )
  }

  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route path="/" element={<RequireAuth><AppShell /></RequireAuth>}>
        <Route index element={<Dashboard />} />
        <Route path="user" element={<UserDashboard />} />
        <Route path="topology" element={<TopologyPage />} />
        <Route path="routes/http" element={<HTTPRouteList />} />
        <Route path="routes/grpc" element={<GRPCRouteList />} />
        <Route path="routes/tcp" element={<TCPRouteList />} />
        <Route path="routes/udp" element={<UDPRouteList />} />
        <Route path="routes/tls" element={<TLSRouteList />} />
        <Route path="infrastructure/gateways" element={<GatewayList />} />
        <Route path="infrastructure/gatewayclasses" element={<GatewayClassList />} />
        <Route path="infrastructure/referencegrants" element={<ReferenceGrantList />} />
        <Route path="services/list" element={<ServiceList />} />
        <Route path="services/endpointslices" element={<EndpointSliceList />} />
        <Route path="services/ai-backends" element={<EdgionBackendList />} />
        <Route path="services/backend-traffic-policies" element={<EdgionBackendTrafficPolicyList />} />
        <Route path="security/tls" element={<EdgionTlsList />} />
        <Route path="security/backendtls" element={<BackendTLSPolicyList />} />
        <Route path="security/dependencies" element={<RestrictedDependenciesPage />} />
        <Route path="plugins" element={<EdgionPluginsList />} />
        <Route path="plugins/stream" element={<EdgionStreamPluginsList />} />
        <Route path="plugins/metadata" element={<EdgionConfigDataList />} />
        <Route path="system/config" element={<EdgionGatewayConfigPage />} />
        <Route path="system/linksys" element={<LinkSysList />} />
        <Route path="system/acme" element={<EdgionAcmeList />} />
        <Route path="region-routes" element={<RegionRouteList />} />
      </Route>
    </Routes>
  )
}

export default App
