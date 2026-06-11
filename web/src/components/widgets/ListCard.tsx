import type { ReactNode } from 'react'
import { StatusDot } from './StatusDot'

interface ListCardRow {
  key: string
  tone: 'success' | 'warning' | 'danger' | 'muted' | 'brand'
  primary: ReactNode
  secondary?: ReactNode
  trailing?: ReactNode
  onClick?: () => void
}

interface ListCardProps {
  title?: ReactNode
  rows: ListCardRow[]
  emptyText?: string
}

export const ListCard = ({ title, rows, emptyText }: ListCardProps) => (
  <div
    style={{
      background: 'var(--ec-color-bg-surface)',
      border: '1px solid var(--ec-color-border)',
      borderRadius: 'var(--ec-radius-md)',
      boxShadow: 'var(--ec-shadow-sm)',
      overflow: 'hidden',
    }}
  >
    {title && (
      <div
        style={{
          padding: '12px 16px',
          fontSize: 'var(--ec-size-md)',
          fontWeight: 600,
          color: 'var(--ec-color-text)',
          borderBottom: '1px solid var(--ec-color-border)',
        }}
      >
        {title}
      </div>
    )}
    {rows.length === 0 ? (
      <div
        style={{
          padding: '24px 16px',
          textAlign: 'center',
          color: 'var(--ec-color-text-muted)',
          fontSize: 'var(--ec-size-sm)',
        }}
      >
        {emptyText ?? 'No items'}
      </div>
    ) : (
      rows.map((row) => (
        <div
          key={row.key}
          onClick={row.onClick}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 12,
            padding: '12px 16px',
            borderTop: '1px solid var(--ec-color-border)',
            cursor: row.onClick ? 'pointer' : 'default',
          }}
        >
          <StatusDot tone={row.tone} />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              style={{
                fontSize: 'var(--ec-size-base)',
                color: 'var(--ec-color-text)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {row.primary}
            </div>
            {row.secondary && (
              <div
                style={{
                  fontSize: 'var(--ec-size-sm)',
                  color: 'var(--ec-color-text-muted)',
                  marginTop: 2,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {row.secondary}
              </div>
            )}
          </div>
          {row.trailing && (
            <div style={{ color: 'var(--ec-color-text-muted)', fontSize: 'var(--ec-size-sm)' }}>
              {row.trailing}
            </div>
          )}
        </div>
      ))
    )}
  </div>
)
