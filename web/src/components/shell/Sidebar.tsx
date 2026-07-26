import type { ReactNode } from 'react'
import { useNavigate, useLocation, useParams } from 'react-router-dom'
import { useT } from '@/i18n'
import { usePermissions } from '@/utils/permissions'
import { useServerInfo } from '@/hooks/useServerInfo'
import {
  filterMenuTree,
  getMenuByMode,
  isMenuNodeActive,
  type AppMode,
  type MenuNode,
} from './menuConfig'
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
  const capabilities = serverInfo?.data?.capabilities ?? {}
  const gateCtx = {
    capabilities,
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

  const renderMenuNode = (node: MenuNode, depth = 0): ReactNode => {
    const label = t(node.labelKey)
    if (node.kind === 'item') {
      return (
        <div
          key={node.key}
          aria-label={collapsed ? label : undefined}
          title={collapsed ? label : undefined}
        >
          <SidebarItem
            label={label}
            icon={node.icon}
            active={isActive(node.path)}
            collapsed={collapsed}
            onClick={() => handleClick(node.path)}
          />
        </div>
      )
    }

    return (
      <SidebarGroup
        key={node.key}
        label={label}
        collapsed={collapsed}
        active={isMenuNodeActive(node, isActive)}
        depth={depth}
      >
        {node.children.map((child) => renderMenuNode(child, depth + 1))}
      </SidebarGroup>
    )
  }

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
        const visibleChildren = filterMenuTree(section.children, gateCtx)
        if (visibleChildren.length === 0) return null
        return (
          <SidebarSection
            key={section.labelKey}
            label={t(section.labelKey)}
            collapsed={collapsed}
            showDivider={sIdx > 0}
          >
            {visibleChildren.map((child) => renderMenuNode(child))}
          </SidebarSection>
        )
      })}
    </aside>
  )
}
