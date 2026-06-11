import type { ReactNode } from 'react'

interface SidebarSectionProps {
  label: string
  collapsed: boolean
  showDivider: boolean
  children: ReactNode
}

export const SidebarSection = ({ label, collapsed, showDivider, children }: SidebarSectionProps) => (
  <div style={{ marginTop: showDivider ? 20 : 8 }}>
    {showDivider && (
      <div
        style={{
          margin: '0 16px 12px',
          height: 1,
          background: 'var(--ec-color-border)',
        }}
      />
    )}
    {!collapsed && (
      <div
        style={{
          padding: '0 20px',
          fontSize: 'var(--ec-size-xs)',
          color: 'var(--ec-color-text-muted)',
          textTransform: 'uppercase',
          letterSpacing: '0.08em',
          fontWeight: 700,
          marginBottom: 8,
        }}
      >
        {label}
      </div>
    )}
    <div>{children}</div>
  </div>
)
