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
  AuditOutlined,
  UserOutlined,
  TeamOutlined,
  CloudOutlined,
} from '@ant-design/icons'

export type AppMode = 'center' | 'controller'
export type CenterCapability = 'userAdmin' | 'roleAdmin' | 'auditQuery' | 'controllerHistory' | 'nativeRbac' | 'leaderElection' | 'passwordLogin' | 'providerAccountAdmin' | 'providerCapabilityRead' | 'providerCredentialInspection' | 'cloudflareDnsRead' | 'cloudflareDnsWrite' | 'cloudflareWafRead' | 'cloudflareWafWrite' | 'route53DnsRead' | 'route53DnsWrite' | 'route53ZoneLifecycle' | 'cloudfrontRead' | 'cloudfrontWrite' | 'awsWafRead' | 'awsWafWrite' | 'awsWafAttach' | 'awsWafDetach' | 'awsWafSecurityWeaken' | 'globalResourcesInventory'

interface MenuGate {
  /** Permission key the caller must hold for this item to be visible. */
  requiredPermission?: string
  /** Additional permission keys; all declared keys are required. */
  requiredPermissions?: string[]
  /** Requires an actually resolved backend capability. */
  requiredCapability?: CenterCapability
}

interface MenuEntry extends MenuGate {
  key: string
  labelKey: string
}

export interface MenuLeaf extends MenuEntry {
  kind: 'item'
  path: string
  icon?: ReactNode
}

export interface MenuBranch extends MenuEntry {
  kind: 'group'
  children: MenuNode[]
}

export type MenuNode = MenuLeaf | MenuBranch

export interface MenuSection {
  kind: 'section'
  labelKey: string
  children: MenuNode[]
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
        key: 'routes',
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
        key: 'services',
        labelKey: 'nav.group.services',
        children: [
          { kind: 'item', key: 'svc-list',     labelKey: 'infra.service',       path: '/services/list',          icon: <DatabaseOutlined /> },
          { kind: 'item', key: 'svc-epslices', labelKey: 'infra.endpointslice', path: '/services/endpointslices',icon: <DatabaseOutlined /> },
          { kind: 'item', key: 'svc-backend-traffic', labelKey: 'services.backendTrafficPolicy', path: '/services/backend-traffic-policies', icon: <DatabaseOutlined /> },
        ],
      },
      {
        kind: 'group',
        key: 'security',
        labelKey: 'nav.group.security',
        children: [
          { kind: 'item', key: 'sec-tls',        labelKey: 'security.tls',        path: '/security/tls',        icon: <SafetyOutlined /> },
          { kind: 'item', key: 'sec-backendtls', labelKey: 'security.backendtls', path: '/security/backendtls', icon: <SafetyOutlined /> },
          { kind: 'item', key: 'sec-dependencies', labelKey: 'security.dependencies', path: '/security/dependencies', icon: <LockOutlined /> },
        ],
      },
      {
        kind: 'group',
        key: 'plugins',
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
        key: 'infrastructure',
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

export const centerMenu: MenuSection[] = [
  {
    kind: 'section',
    labelKey: 'center.nav.section.federation',
    children: [
      { kind: 'item', key: 'center-dashboard', labelKey: 'center.nav.dashboard',
        path: '/', icon: <DashboardOutlined /> },
      { kind: 'item', key: 'center-controllers', labelKey: 'center.nav.controllers',
        path: '/controllers', icon: <ClusterOutlined />, requiredPermission: 'controllers:read' },
    ],
  },
  {
    kind: 'section',
    labelKey: 'center.nav.section.traffic',
    children: [
      { kind: 'item', key: 'center-region-routes', labelKey: 'center.nav.regionRoutes', path: '/region-routes/region', icon: <ShareAltOutlined />, requiredPermission: 'region-routes:read' },
      {
        kind: 'group',
        key: 'center-global-resources',
        labelKey: 'center.nav.globalResources',
        requiredPermission: 'global-resources:read',
        requiredCapability: 'globalResourcesInventory',
        children: [
          { kind: 'item', key: 'center-global-config-data-ip-list', labelKey: 'center.nav.configData.ipList',
            path: '/global-resources/edgion-config-data/ip-list', icon: <DatabaseOutlined /> },
          { kind: 'item', key: 'center-global-config-data-key-list', labelKey: 'center.nav.configData.keyList',
            path: '/global-resources/edgion-config-data/key-list', icon: <DatabaseOutlined /> },
          { kind: 'item', key: 'center-global-config-data-selector', labelKey: 'center.nav.configData.selector',
            path: '/global-resources/edgion-config-data/selector', icon: <DatabaseOutlined /> },
          { kind: 'item', key: 'center-global-config-data-misc', labelKey: 'center.nav.configData.misc',
            path: '/global-resources/edgion-config-data/misc', icon: <DatabaseOutlined /> },
        ],
      },
    ],
  },
  {
    kind: 'section',
    labelKey: 'center.nav.section.cloud',
    children: [
      { kind: 'item', key: 'center-cloud-accounts', labelKey: 'cloud.nav.accounts',
        path: '/cloud/provider-accounts', icon: <CloudOutlined />, requiredPermission: 'provider-accounts:read', requiredCapability: 'providerAccountAdmin' },
      {
        kind: 'group',
        key: 'center-cloudflare',
        labelKey: 'cloud.nav.cloudflare',
        children: [
          { kind: 'item', key: 'center-cloudflare-dns', labelKey: 'cloud.nav.cloudflareDns', path: '/cloud/cloudflare/dns', icon: <CloudOutlined />, requiredPermissions: ['cloudflare-dns:read', 'provider-accounts:read'], requiredCapability: 'cloudflareDnsRead' },
          { kind: 'item', key: 'center-cloudflare-waf', labelKey: 'cloud.nav.cloudflareWaf', path: '/cloud/cloudflare/waf', icon: <SafetyOutlined />, requiredPermissions: ['cloudflare-waf:read', 'cloudflare-dns:read', 'provider-accounts:read'], requiredCapability: 'cloudflareWafRead' },
        ],
      },
      {
        kind: 'group',
        key: 'center-aws',
        labelKey: 'cloud.nav.aws',
        children: [
          { kind: 'item', key: 'center-aws-route53', labelKey: 'cloud.nav.route53', path: '/cloud/aws/route53', icon: <CloudOutlined />, requiredPermissions: ['route53-dns:read', 'provider-accounts:read'], requiredCapability: 'route53DnsRead' },
          { kind: 'item', key: 'center-aws-cloudfront', labelKey: 'cloud.nav.cloudfront', path: '/cloud/aws/cloudfront', icon: <CloudOutlined />, requiredPermissions: ['cloudfront:read', 'provider-accounts:read'], requiredCapability: 'cloudfrontRead' },
          { kind: 'item', key: 'center-aws-waf', labelKey: 'cloud.nav.awsWaf', path: '/cloud/aws/waf', icon: <SafetyOutlined />, requiredPermissions: ['aws-waf:read', 'provider-accounts:read'], requiredCapability: 'awsWafRead' },
        ],
      },
    ],
  },
  {
    kind: 'section',
    labelKey: 'center.nav.section.system',
    children: [
      { kind: 'item', key: 'center-audit', labelKey: 'center.nav.audit',
        path: '/audit', icon: <AuditOutlined />, requiredPermission: 'audit:read', requiredCapability: 'auditQuery' },
      { kind: 'item', key: 'center-users', labelKey: 'center.nav.users',
        path: '/users', icon: <UserOutlined />,
        requiredPermission: 'users:manage', requiredCapability: 'userAdmin' },
      { kind: 'item', key: 'center-roles', labelKey: 'center.nav.roles',
        path: '/roles', icon: <TeamOutlined />,
        requiredPermission: 'roles:manage', requiredCapability: 'roleAdmin' },
    ],
  },
]

export const getMenuByMode = (mode: AppMode): MenuSection[] =>
  mode === 'center' ? centerMenu : controllerMenu

/** Context the menu-visibility predicate evaluates an item against. */
export interface MenuGateContext {
  capabilities: Partial<Record<CenterCapability, boolean>>
  permissions: string[]
}

/**
 * An item is visible IFF every gate it declares is satisfied (AND semantics):
 * the required capability must be resolved and `requiredPermission` must be held.
 * Items carrying no gates are always visible, so existing menu entries are
 * unaffected.
 */
export const isMenuItemVisible = (
  item: MenuGate,
  ctx: MenuGateContext,
): boolean => {
  if (item.requiredCapability && ctx.capabilities[item.requiredCapability] !== true) return false
  if (item.requiredPermission && !ctx.permissions.includes(item.requiredPermission)) return false
  if (item.requiredPermissions?.some((permission) => !ctx.permissions.includes(permission))) return false
  return true
}

/** Recursively applies access gates and removes branches with no visible descendants. */
export const filterMenuTree = (
  nodes: MenuNode[],
  ctx: MenuGateContext,
): MenuNode[] => nodes.reduce<MenuNode[]>((visible, node) => {
  if (!isMenuItemVisible(node, ctx)) return visible
  if (node.kind === 'item') {
    visible.push(node)
    return visible
  }

  const children = filterMenuTree(node.children, ctx)
  if (children.length > 0) visible.push({ ...node, children })
  return visible
}, [])

/** Reports whether this node or any descendant resolves to the active route. */
export const isMenuNodeActive = (
  node: MenuNode,
  isPathActive: (path: string) => boolean,
): boolean => node.kind === 'item'
  ? isPathActive(node.path)
  : node.children.some((child) => isMenuNodeActive(child, isPathActive))
