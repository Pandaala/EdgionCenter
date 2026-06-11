import type { ReactNode } from 'react'

interface PageHeaderProps {
  title: ReactNode
  subtitle?: ReactNode
  actions?: ReactNode
  /** Back-compat aliases used by older call sites. */
  description?: ReactNode
  extra?: ReactNode
}

const PageHeader = ({ title, subtitle, actions, description, extra }: PageHeaderProps) => {
  const sub = subtitle ?? description
  const right = actions ?? extra
  return (
    <div
      style={{
        marginBottom: 24,
        display: 'flex',
        alignItems: 'flex-start',
        gap: 16,
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <h1
          style={{
            margin: 0,
            fontSize: 'var(--ec-size-2xl)',
            fontWeight: 600,
            color: 'var(--ec-color-text)',
            letterSpacing: '-0.01em',
          }}
        >
          {title}
        </h1>
        {sub && (
          <p
            style={{
              margin: '4px 0 0',
              fontSize: 'var(--ec-size-md)',
              color: 'var(--ec-color-text-muted)',
            }}
          >
            {sub}
          </p>
        )}
      </div>
      {right && <div style={{ display: 'flex', gap: 8 }}>{right}</div>}
    </div>
  )
}

export default PageHeader
