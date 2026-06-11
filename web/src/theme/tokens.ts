export type ThemeMode = 'light' | 'dark' | 'system'
export type ResolvedMode = 'light' | 'dark'

export interface ThemeTokens {
  color: {
    brand: string
    brandSoftBg: string
    brandSoftText: string
    bgBase: string
    bgSurface: string
    bgSubtle: string
    bgHover: string
    border: string
    borderStrong: string
    text: string
    textMuted: string
    textSubtle: string
    success: string
    warning: string
    danger: string
    info: string
  }
  shadow: { sm: string; md: string; lg: string }
  radius: { sm: number; md: number; lg: number }
  font: { sans: string; mono: string }
  spacing: { 1: 4; 2: 8; 3: 12; 4: 16; 5: 20; 6: 24; 8: 32; 12: 48 }
  size: {
    xs: 11; sm: 12; base: 13; md: 14; lg: 16; xl: 18; '2xl': 22; '3xl': 28
  }
}

const sansStack =
  "'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
const monoStack =
  "'JetBrains Mono', 'SF Mono', Menlo, Consolas, monospace"

const sharedRadius = { sm: 6, md: 8, lg: 12 } as const
const sharedSpacing = { 1: 4, 2: 8, 3: 12, 4: 16, 5: 20, 6: 24, 8: 32, 12: 48 } as const
const sharedSize = {
  xs: 11, sm: 12, base: 13, md: 14, lg: 16, xl: 18, '2xl': 22, '3xl': 28,
} as const
const sharedFont = { sans: sansStack, mono: monoStack } as const

export const lightTokens: ThemeTokens = {
  color: {
    brand: '#0891B2',
    brandSoftBg: '#ECFEFF',
    brandSoftText: '#0891B2',
    bgBase: '#FFFFFF',
    bgSurface: '#FFFFFF',
    bgSubtle: '#F8FAFC',
    bgHover: '#F3F4F6',
    border: '#E5E7EB',
    borderStrong: '#D1D5DB',
    text: '#0F172A',
    textMuted: '#6B7280',
    textSubtle: '#9CA3AF',
    success: '#10B981',
    warning: '#F59E0B',
    danger: '#EF4444',
    info: '#0891B2',
  },
  shadow: {
    sm: '0 1px 2px rgba(0,0,0,0.04)',
    md: '0 4px 12px rgba(0,0,0,0.06)',
    lg: '0 12px 32px rgba(0,0,0,0.08)',
  },
  radius: sharedRadius,
  font: sharedFont,
  spacing: sharedSpacing,
  size: sharedSize,
}

export const darkTokens: ThemeTokens = {
  color: {
    brand: '#22D3EE',
    brandSoftBg: '#0E2A33',
    brandSoftText: '#67E8F9',
    bgBase: '#0B1220',
    bgSurface: '#111827',
    bgSubtle: '#0F172A',
    bgHover: '#1E293B',
    border: '#1F2937',
    borderStrong: '#334155',
    text: '#F1F5F9',
    textMuted: '#94A3B8',
    textSubtle: '#64748B',
    success: '#34D399',
    warning: '#FBBF24',
    danger: '#F87171',
    info: '#22D3EE',
  },
  shadow: {
    sm: '0 1px 2px rgba(0,0,0,0.4)',
    md: '0 4px 12px rgba(0,0,0,0.4)',
    lg: '0 12px 32px rgba(0,0,0,0.5)',
  },
  radius: sharedRadius,
  font: sharedFont,
  spacing: sharedSpacing,
  size: sharedSize,
}

export const tokensFor = (m: ResolvedMode): ThemeTokens =>
  m === 'dark' ? darkTokens : lightTokens
