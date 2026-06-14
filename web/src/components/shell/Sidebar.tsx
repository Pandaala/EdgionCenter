import { useNavigate, useLocation, useParams } from 'react-router-dom'
import { useT } from '@/i18n'
import { usePermissions } from '@/utils/permissions'
import { useServerInfo } from '@/hooks/useServerInfo'
import { getMenuByMode, isMenuItemVisible, type AppMode, type MenuGroup, type MenuLeaf } from './menuConfig'
import { SidebarSection } from './SidebarSection'
import { SidebarGroup } from './SidebarGroup'
import { SidebarItem } from './SidebarItem'

interface SidebarProps {
  collapsed: boolean
  mode?: AppMode
}

export const Sidebar = ({ collapsed, mode = 'controller' }: SidebarProps) => {
  const menuConfig = getMenuByMode(mode)
  const navigate = useNavigate()
  const location = useLocation()
  const t = useT()
  const { permissions } = usePermissions()
  const { data: serverInfo } = useServerInfo()
  const authzMode = serverInfo?.data?.authzMode
  const dbAuthEnabled = serverInfo?.data?.dbAuthEnabled ?? false
  const gateCtx = {
    authzMode,
    dbAuthEnabled,
    // The users table is in use under rbac, or when DB-backed auth is enabled.
    userMgmtAvailable: authzMode === 'rbac' || dbAuthEnabled,
    permissions,
  }
  const { controllerId: rawId } = useParams<{ controllerId?: string }>()
  const activeControllerId = rawId?.replace(/~/g, '/') ?? null
  const prefix = activeControllerId
    ? `/controller/${activeControllerId.replace(/\//g, '~')}`
    : ''

  const effectivePath = (() => {
    let p = location.pathname
    if (prefix && p.startsWith(prefix)) p = p.slice(prefix.length) || '/'
    return p
  })()

  const isActive = (path: string) => {
    if (path === '/') return effectivePath === '/'
    return effectivePath === path
  }

  const handleClick = (path: string) => navigate(`${prefix}${path}`)

  return (
    <aside
      style={{
        width: collapsed ? 64 : 240,
        flexShrink: 0,
        background: 'var(--ec-color-bg-subtle)',
        borderRight: '1px solid var(--ec-color-border)',
        height: '100vh',
        overflowY: 'auto',
        paddingBottom: 24,
        transition: 'width 120ms ease',
      }}
    >
      {menuConfig.map((section, sIdx) => {
        // Filter leaves by the access-mode + permission gate; drop groups that
        // end up empty so we never render a header with no items.
        const visibleChildren = section.children
          .map((child): MenuLeaf | MenuGroup | null => {
            if (child.kind === 'item') {
              return isMenuItemVisible(child, gateCtx) ? child : null
            }
            const leaves = child.children.filter((leaf) => isMenuItemVisible(leaf, gateCtx))
            return leaves.length > 0 ? { ...child, children: leaves } : null
          })
          .filter((c): c is MenuLeaf | MenuGroup => c !== null)
        if (visibleChildren.length === 0) return null
        return (
          <SidebarSection
            key={section.labelKey}
            label={t(section.labelKey)}
            collapsed={collapsed}
            showDivider={sIdx > 0}
          >
            {visibleChildren.map((child) => {
              if (child.kind === 'item') {
                return (
                <SidebarItem
                  key={child.key}
                  label={t(child.labelKey)}
                  icon={child.icon}
                  active={isActive(child.path)}
                  collapsed={collapsed}
                  onClick={() => handleClick(child.path)}
                />
              )
            }
            return (
              <SidebarGroup key={child.labelKey} label={t(child.labelKey)} collapsed={collapsed}>
                {child.children.map((leaf) => (
                  <SidebarItem
                    key={leaf.key}
                    label={t(leaf.labelKey)}
                    icon={leaf.icon}
                    active={isActive(leaf.path)}
                    collapsed={collapsed}
                    onClick={() => handleClick(leaf.path)}
                  />
                ))}
              </SidebarGroup>
            )
            })}
          </SidebarSection>
        )
      })}
    </aside>
  )
}
