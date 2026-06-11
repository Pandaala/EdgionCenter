import { useNavigate, useLocation, useParams } from 'react-router-dom'
import { useT } from '@/i18n'
import { getMenuByMode, type AppMode } from './menuConfig'
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
      {menuConfig.map((section, sIdx) => (
        <SidebarSection
          key={section.labelKey}
          label={t(section.labelKey)}
          collapsed={collapsed}
          showDivider={sIdx > 0}
        >
          {section.children.map((child) => {
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
      ))}
    </aside>
  )
}
