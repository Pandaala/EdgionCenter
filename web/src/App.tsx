import { useState, useEffect } from 'react'
import { Routes, Route, Navigate } from 'react-router-dom'
import { Spin } from 'antd'
import { AppShell } from './components/shell/AppShell'
import ControllerProxy from './components/Layout/ControllerProxy'
import { isLoggedIn } from './utils/auth'
import { setAppMode } from './utils/proxy'
import { systemApi } from './api/client'
import LoginPage from './pages/Login/LoginPage'
import Dashboard from './pages/Dashboard'
import UserDashboard from './pages/Dashboard/UserDashboard'
import CenterDashboard from './pages/Center/CenterDashboard'
import CenterAdminPage from './pages/Center/CenterAdminPage'
// RegionRoute
import ClusterRegionRouteList from './pages/RegionRoute/ClusterRegionRouteList'
import ServiceRegionRouteList from './pages/RegionRoute/ServiceRegionRouteList'
import ControllerClusterRegionRouteList from './pages/RegionRoute/ControllerClusterRegionRouteList'
import ControllerServiceRegionRouteList from './pages/RegionRoute/ControllerServiceRegionRouteList'
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
import ReferenceGrantList from './pages/Infrastructure/ReferenceGrantList'
// Security
import EdgionTlsList from './pages/Security/EdgionTlsList'
import BackendTLSPolicyList from './pages/Security/BackendTLSPolicyList'
// Plugins
import EdgionPluginsList from './pages/Plugins/EdgionPluginsList'
import EdgionStreamPluginsList from './pages/Plugins/EdgionStreamPluginsList'
import PluginMetaDataList from './pages/Plugins/PluginMetaDataList'
// System
import EdgionGatewayConfigPage from './pages/System/EdgionGatewayConfigPage'
import LinkSysList from './pages/System/LinkSysList'
import EdgionAcmeList from './pages/System/EdgionAcmeList'
import TopologyPage from './pages/Topology/TopologyPage'
import GlobalConnectionIpRestrictionList from './pages/GlobalConnectionIpRestriction/List'
import GlobalConnectionIpRestrictionDetail from './pages/GlobalConnectionIpRestriction/Detail'
import './App.css'

function RequireAuth({ children }: { children: React.ReactNode }) {
  if (!isLoggedIn()) {
    return <Navigate to="/login" replace />
  }
  return <>{children}</>
}

function App() {
  const [mode, setMode] = useState<'controller' | 'center' | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    // server-info is unauthenticated — always call it to detect mode
    systemApi
      .serverInfo()
      .then((res) => {
        const m = res.data?.mode === 'center' ? 'center' : 'controller'
        setMode(m)
        setAppMode(m)
      })
      .catch(() => {
        setMode('controller')
        setAppMode('controller')
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

  if (mode === 'center') {
    return (
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/" element={<RequireAuth><AppShell mode="center" /></RequireAuth>}>
          <Route index element={<CenterDashboard />} />
          <Route path="region-routes/cluster" element={<ClusterRegionRouteList />} />
          <Route path="region-routes/service" element={<ServiceRegionRouteList />} />
          <Route
            path="global-connection-ip-restrictions"
            element={<GlobalConnectionIpRestrictionList />}
          />
          <Route
            path="global-connection-ip-restrictions/:namespace/:name/:controllerId"
            element={<GlobalConnectionIpRestrictionDetail />}
          />
          <Route path="admin" element={<CenterAdminPage />} />
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
          <Route path="security/tls" element={<EdgionTlsList />} />
          <Route path="security/backendtls" element={<BackendTLSPolicyList />} />
          <Route path="plugins" element={<EdgionPluginsList />} />
          <Route path="plugins/stream" element={<EdgionStreamPluginsList />} />
          <Route path="plugins/metadata" element={<PluginMetaDataList />} />
          <Route path="system/config" element={<EdgionGatewayConfigPage />} />
          <Route path="system/linksys" element={<LinkSysList />} />
          <Route path="system/acme" element={<EdgionAcmeList />} />
          <Route path="region-routes/cluster" element={<ControllerClusterRegionRouteList />} />
          <Route path="region-routes/service" element={<ControllerServiceRegionRouteList />} />
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
        <Route path="security/tls" element={<EdgionTlsList />} />
        <Route path="security/backendtls" element={<BackendTLSPolicyList />} />
        <Route path="plugins" element={<EdgionPluginsList />} />
        <Route path="plugins/stream" element={<EdgionStreamPluginsList />} />
        <Route path="plugins/metadata" element={<PluginMetaDataList />} />
        <Route path="system/config" element={<EdgionGatewayConfigPage />} />
        <Route path="system/linksys" element={<LinkSysList />} />
        <Route path="system/acme" element={<EdgionAcmeList />} />
        <Route path="region-routes/cluster" element={<ControllerClusterRegionRouteList />} />
        <Route path="region-routes/service" element={<ControllerServiceRegionRouteList />} />
      </Route>
    </Routes>
  )
}

export default App
