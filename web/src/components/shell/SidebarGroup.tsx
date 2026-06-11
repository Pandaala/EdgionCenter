import type { ReactNode } from 'react'

interface SidebarGroupProps {
  label: string
  collapsed: boolean
  children: ReactNode
}

export const SidebarGroup = ({ label, collapsed, children }: SidebarGroupProps) => (
  <div style={{ marginTop: 12 }}>
    {!collapsed && (
      <div
        style={{
          padding: '0 20px',
          fontSize: 'var(--ec-size-xs)',
          color: 'var(--ec-color-text-subtle)',
          textTransform: 'uppercase',
          letterSpacing: '0.06em',
          fontWeight: 600,
          marginBottom: 6,
        }}
      >
        {label}
      </div>
    )}
    <div>{children}</div>
  </div>
)
