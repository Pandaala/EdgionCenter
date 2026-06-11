import type { ReactNode, MouseEvent } from 'react'

interface SidebarItemProps {
  label: string
  icon?: ReactNode
  active: boolean
  collapsed: boolean
  onClick: (e: MouseEvent) => void
}

export const SidebarItem = ({ label, icon, active, collapsed, onClick }: SidebarItemProps) => {
  const bg = active ? 'var(--ec-color-brand-soft-bg)' : 'transparent'
  const color = active ? 'var(--ec-color-brand-soft-text)' : 'var(--ec-color-text)'
  const fontWeight = active ? 500 : 400
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onClick(e as unknown as MouseEvent) }}
      title={collapsed ? label : undefined}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        height: 32,
        margin: '2px 8px',
        padding: collapsed ? '0 0' : '0 12px',
        borderRadius: 'var(--ec-radius-sm)',
        background: bg,
        color,
        fontWeight,
        fontSize: 'var(--ec-size-base)',
        cursor: 'pointer',
        userSelect: 'none',
        justifyContent: collapsed ? 'center' : 'flex-start',
        transition: 'background-color 80ms ease, color 80ms ease',
      }}
      onMouseEnter={(e) => { if (!active) (e.currentTarget as HTMLDivElement).style.background = 'var(--ec-color-bg-hover)' }}
      onMouseLeave={(e) => { if (!active) (e.currentTarget as HTMLDivElement).style.background = 'transparent' }}
    >
      {icon && <span style={{ display: 'inline-flex', fontSize: 14 }}>{icon}</span>}
      {!collapsed && <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{label}</span>}
    </div>
  )
}
