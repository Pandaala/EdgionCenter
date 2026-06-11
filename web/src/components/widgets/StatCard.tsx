import type { ReactNode } from 'react'

interface StatCardProps {
  label: string
  value: ReactNode
  delta?: { value: number; suffix?: string; tone?: 'success' | 'danger' | 'muted' }
  icon?: ReactNode
}

export const StatCard = ({ label, value, delta, icon }: StatCardProps) => {
  const deltaColor =
    delta?.tone === 'danger'
      ? 'var(--ec-color-danger)'
      : delta?.tone === 'muted'
      ? 'var(--ec-color-text-muted)'
      : 'var(--ec-color-success)'
  return (
    <div
      style={{
        background: 'var(--ec-color-bg-surface)',
        border: '1px solid var(--ec-color-border)',
        borderRadius: 'var(--ec-radius-md)',
        padding: 16,
        boxShadow: 'var(--ec-shadow-sm)',
        display: 'flex',
        gap: 12,
        alignItems: 'flex-start',
      }}
    >
      {icon && (
        <div
          style={{
            width: 36,
            height: 36,
            borderRadius: 'var(--ec-radius-sm)',
            background: 'var(--ec-color-brand-soft-bg)',
            color: 'var(--ec-color-brand-soft-text)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            fontSize: 16,
          }}
        >
          {icon}
        </div>
      )}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontSize: 'var(--ec-size-xs)',
            color: 'var(--ec-color-text-muted)',
            textTransform: 'uppercase',
            letterSpacing: '0.06em',
            fontWeight: 500,
          }}
        >
          {label}
        </div>
        <div
          style={{
            fontSize: 'var(--ec-size-3xl)',
            fontWeight: 600,
            color: 'var(--ec-color-text)',
            marginTop: 4,
            letterSpacing: '-0.01em',
            lineHeight: 1.1,
          }}
        >
          {value}
        </div>
        {delta && (
          <div
            style={{
              fontSize: 'var(--ec-size-xs)',
              color: deltaColor,
              marginTop: 4,
            }}
          >
            {delta.value >= 0 ? `+${delta.value}` : delta.value}
            {delta.suffix ? ` ${delta.suffix}` : ''}
          </div>
        )}
      </div>
    </div>
  )
}
