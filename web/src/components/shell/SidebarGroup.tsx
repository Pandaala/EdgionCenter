import type { ReactNode } from 'react'

interface SidebarGroupProps {
  label: string
  collapsed: boolean
  active: boolean
  depth: number
  children: ReactNode
}

export const SidebarGroup = ({ label, collapsed, active, depth, children }: SidebarGroupProps) => (
  <div
    aria-label={collapsed ? label : undefined}
    title={collapsed ? label : undefined}
    style={{
      marginTop: depth === 0 ? 12 : 6,
      marginLeft: collapsed ? 0 : depth * 10,
      borderLeft: collapsed || depth === 0
        ? undefined
        : '1px solid var(--ec-color-border)',
    }}
  >
    {!collapsed && (
      <div
        style={{
          padding: '0 20px',
          fontSize: 'var(--ec-size-xs)',
          color: active
            ? 'var(--ec-color-brand-soft-text)'
            : 'var(--ec-color-text-subtle)',
          textTransform: 'uppercase',
          letterSpacing: '0.06em',
          fontWeight: active ? 700 : 600,
          marginBottom: 6,
        }}
      >
        {label}
      </div>
    )}
    <div>{children}</div>
  </div>
)
