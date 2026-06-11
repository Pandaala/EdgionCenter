type Tone = 'success' | 'warning' | 'danger' | 'muted' | 'brand'

interface StatusDotProps {
  tone: Tone
  size?: number
}

const colorVar = (tone: Tone) => {
  switch (tone) {
    case 'success': return 'var(--ec-color-success)'
    case 'warning': return 'var(--ec-color-warning)'
    case 'danger':  return 'var(--ec-color-danger)'
    case 'brand':   return 'var(--ec-color-brand)'
    case 'muted':   return 'var(--ec-color-text-subtle)'
  }
}

export const StatusDot = ({ tone, size = 8 }: StatusDotProps) => (
  <span
    aria-hidden
    style={{
      display: 'inline-block',
      width: size,
      height: size,
      borderRadius: '50%',
      background: colorVar(tone),
      flexShrink: 0,
    }}
  />
)
