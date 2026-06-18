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
  AuditOutlined,
  UserOutlined,
  TeamOutlined,
} from '@ant-design/icons'
import type { AuthzMode } from '@/hooks/useServerInfo'

export type AppMode = 'center' | 'controller'

export interface MenuLeaf {
  kind: 'item'
  key: string
  labelKey: string
  path: string
  icon?: ReactNode
  /** Permission key the caller must hold for this item to be visible. */
  requiredPermission?: string
  /** Requires the authorization mode to be exactly this. Used to gate the Roles
   *  / permission-matrix page on `rbac` (under `allow_all` everyone implicitly
   *  has every permission, so the keys alone cannot distinguish the modes). */
  requiredAuthz?: AuthzMode
  /** Requires the user-management surface to be in use, i.e. the `users` table
   *  is backing auth (`authzMode === 'rbac' || dbAuthEnabled`). Gates the Users
   *  page, which would otherwise be meaningless when DB-backed users are off. */
  requiredUserMgmt?: true
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
      { kind: 'item', key: 'rr', labelKey: 'nav.regionRoutes', path: '/region-routes', icon: <ShareAltOutlined /> },
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
      { kind: 'item', key: 'center-rr', labelKey: 'center.nav.regionRoutes', path: '/region-routes', icon: <ShareAltOutlined /> },
      { kind: 'item', key: 'center-gipr', labelKey: 'center.nav.globalIpRestrictions',
        path: '/global-connection-ip-restrictions', icon: <SafetyOutlined /> },
      { kind: 'item', key: 'center-admin', labelKey: 'center.nav.admin',
        path: '/admin', icon: <SettingOutlined /> },
      { kind: 'item', key: 'center-audit', labelKey: 'center.nav.audit',
        path: '/audit', icon: <AuditOutlined />, requiredPermission: 'audit:read' },
      { kind: 'item', key: 'center-users', labelKey: 'center.nav.users',
        path: '/users', icon: <UserOutlined />,
        requiredPermission: 'users:manage', requiredUserMgmt: true },
      { kind: 'item', key: 'center-roles', labelKey: 'center.nav.roles',
        path: '/roles', icon: <TeamOutlined />,
        requiredPermission: 'roles:manage', requiredAuthz: 'rbac' },
    ],
  },
]

export const getMenuByMode = (mode: AppMode): MenuSection[] =>
  mode === 'center' ? centerMenu : controllerMenu

/** Context the menu-visibility predicate evaluates an item against. */
export interface MenuGateContext {
  /** Current authorization mode reported by `/server-info`. */
  authzMode: AuthzMode | undefined
  /** Whether DB-backed (table `users`) authentication is enabled. */
  dbAuthEnabled: boolean
  /** Derived: the user-management surface is in use when authz is `rbac` OR
   *  DB-backed users are enabled. Encodes the "users table in use" OR-gate so
   *  the predicate can stay a plain AND of independent checks. */
  userMgmtAvailable: boolean
  permissions: string[]
}

/**
 * An item is visible IFF every gate it declares is satisfied (AND semantics):
 * `requiredAuthz` must equal `ctx.authzMode`, `requiredUserMgmt` requires
 * `ctx.userMgmtAvailable`, and `requiredPermission` must be held by the caller.
 * Items carrying no gates are always visible, so existing menu entries are
 * unaffected.
 */
export const isMenuItemVisible = (
  item: { requiredPermission?: string; requiredAuthz?: AuthzMode; requiredUserMgmt?: true },
  ctx: MenuGateContext,
): boolean => {
  if (item.requiredAuthz && ctx.authzMode !== item.requiredAuthz) return false
  if (item.requiredUserMgmt && !ctx.userMgmtAvailable) return false
  if (item.requiredPermission && !ctx.permissions.includes(item.requiredPermission)) return false
  return true
}
