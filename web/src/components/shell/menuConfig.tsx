import type { ReactNode } from 'react'
import {
  DashboardOutlined,
  ApartmentOutlined,
  ApiOutlined,
  DatabaseOutlined,
  SafetyOutlined,
  AppstoreOutlined,
  ShareAltOutlined,
  ClusterOutlined,
  LinkOutlined,
  LockOutlined,
  SettingOutlined,
} from '@ant-design/icons'

export type AppMode = 'center' | 'controller'

export interface MenuLeaf {
  kind: 'item'
  key: string
  labelKey: string
  path: string
  icon?: ReactNode
}

export interface MenuGroup {
  kind: 'group'
  labelKey: string
  children: MenuLeaf[]
}

export interface MenuSection {
  kind: 'section'
  labelKey: string
  children: (MenuGroup | MenuLeaf)[]
}

export const controllerMenu: MenuSection[] = [
  {
    kind: 'section',
    labelKey: 'nav.section.user',
    children: [
      { kind: 'item', key: 'user-dashboard', labelKey: 'nav.dashboard',
        path: '/user', icon: <DashboardOutlined /> },
      { kind: 'item', key: 'topology', labelKey: 'nav.topology',
        path: '/topology', icon: <ApartmentOutlined /> },
      {
        kind: 'group',
        labelKey: 'nav.group.routes',
        children: [
          { kind: 'item', key: 'route-http', labelKey: 'route.http', path: '/routes/http', icon: <ApiOutlined /> },
          { kind: 'item', key: 'route-grpc', labelKey: 'route.grpc', path: '/routes/grpc', icon: <ApiOutlined /> },
          { kind: 'item', key: 'route-tcp',  labelKey: 'route.tcp',  path: '/routes/tcp',  icon: <ApiOutlined /> },
          { kind: 'item', key: 'route-udp',  labelKey: 'route.udp',  path: '/routes/udp',  icon: <ApiOutlined /> },
          { kind: 'item', key: 'route-tls',  labelKey: 'route.tls',  path: '/routes/tls',  icon: <ApiOutlined /> },
        ],
      },
      {
        kind: 'group',
        labelKey: 'nav.group.services',
        children: [
          { kind: 'item', key: 'svc-list',     labelKey: 'infra.service',       path: '/services/list',          icon: <DatabaseOutlined /> },
          { kind: 'item', key: 'svc-epslices', labelKey: 'infra.endpointslice', path: '/services/endpointslices',icon: <DatabaseOutlined /> },
        ],
      },
      {
        kind: 'group',
        labelKey: 'nav.group.security',
        children: [
          { kind: 'item', key: 'sec-tls',        labelKey: 'security.tls',        path: '/security/tls',        icon: <SafetyOutlined /> },
          { kind: 'item', key: 'sec-backendtls', labelKey: 'security.backendtls', path: '/security/backendtls', icon: <SafetyOutlined /> },
        ],
      },
      {
        kind: 'group',
        labelKey: 'nav.group.plugins',
        children: [
          { kind: 'item', key: 'plg-edgion', labelKey: 'plugins.edgion',  path: '/plugins',          icon: <AppstoreOutlined /> },
          { kind: 'item', key: 'plg-stream', labelKey: 'plugins.stream',  path: '/plugins/stream',   icon: <AppstoreOutlined /> },
          { kind: 'item', key: 'plg-meta',   labelKey: 'plugins.metadata',path: '/plugins/metadata', icon: <AppstoreOutlined /> },
        ],
      },
      { kind: 'item', key: 'refgrant', labelKey: 'infra.referencegrant',
        path: '/infrastructure/referencegrants', icon: <ShareAltOutlined /> },
    ],
  },
  {
    kind: 'section',
    labelKey: 'nav.section.ops',
    children: [
      { kind: 'item', key: 'ops-dashboard', labelKey: 'nav.dashboard',
        path: '/', icon: <DashboardOutlined /> },
      {
        kind: 'group',
        labelKey: 'nav.group.infrastructure',
        children: [
          { kind: 'item', key: 'infra-gw',       labelKey: 'infra.gateway',      path: '/infrastructure/gateways',       icon: <ClusterOutlined /> },
          { kind: 'item', key: 'infra-gwclass',  labelKey: 'infra.gatewayclass', path: '/infrastructure/gatewayclasses', icon: <ClusterOutlined /> },
          { kind: 'item', key: 'infra-config',   labelKey: 'system.config',      path: '/system/config',                 icon: <ClusterOutlined /> },
        ],
      },
      { kind: 'item', key: 'sys-linksys', labelKey: 'system.linksys', path: '/system/linksys', icon: <LinkOutlined /> },
      { kind: 'item', key: 'sys-acme',    labelKey: 'system.acme',    path: '/system/acme',    icon: <LockOutlined /> },
      {
        kind: 'group',
        labelKey: 'nav.group.regionRoutes',
        children: [
          { kind: 'item', key: 'rr-cluster', labelKey: 'nav.regionRouteCluster', path: '/region-routes/cluster', icon: <ShareAltOutlined /> },
          { kind: 'item', key: 'rr-service', labelKey: 'nav.regionRouteService', path: '/region-routes/service', icon: <ShareAltOutlined /> },
        ],
      },
    ],
  },
]

// Single flat section: Center has no User/Ops split — it manages a fleet of controllers.
export const centerMenu: MenuSection[] = [
  {
    kind: 'section',
    labelKey: 'center.title',
    children: [
      { kind: 'item', key: 'center-controllers', labelKey: 'center.nav.controllers',
        path: '/', icon: <ClusterOutlined /> },
      {
        kind: 'group',
        labelKey: 'center.nav.regionRoutes',
        children: [
          { kind: 'item', key: 'center-rr-cluster', labelKey: 'center.nav.region',
            path: '/region-routes/cluster', icon: <ShareAltOutlined /> },
          { kind: 'item', key: 'center-rr-service', labelKey: 'center.nav.servicePm',
            path: '/region-routes/service', icon: <ShareAltOutlined /> },
        ],
      },
      { kind: 'item', key: 'center-gipr', labelKey: 'center.nav.globalIpRestrictions',
        path: '/global-connection-ip-restrictions', icon: <SafetyOutlined /> },
      { kind: 'item', key: 'center-admin', labelKey: 'center.nav.admin',
        path: '/admin', icon: <SettingOutlined /> },
    ],
  },
]

export const getMenuByMode = (mode: AppMode): MenuSection[] =>
  mode === 'center' ? centerMenu : controllerMenu
