import { useEffect, useMemo, useState, createContext, useCallback } from 'react'
import type { ReactNode } from 'react'
import { ConfigProvider } from 'antd'
import enUS from 'antd/es/locale/en_US'
import zhCN from 'antd/es/locale/zh_CN'
import { useLanguage } from '@/i18n'
import { lightTokens, tokensFor } from './tokens'
import type { ThemeMode, ResolvedMode, ThemeTokens } from './tokens'
import { tokensToCssVars, applyCssVars } from './cssVars'
import { buildAntdTheme } from './antdTheme'

const STORAGE_KEY = 'edgion.theme.mode'

interface ThemeContextValue {
  mode: ThemeMode
  resolvedMode: ResolvedMode
  setMode: (m: ThemeMode) => void
  tokens: ThemeTokens
}

export const ThemeContext = createContext<ThemeContextValue>({
  mode: 'system',
  resolvedMode: 'light',
  setMode: () => {},
  tokens: lightTokens,
})

const readStoredMode = (): ThemeMode => {
  const v = localStorage.getItem(STORAGE_KEY)
  return v === 'light' || v === 'dark' || v === 'system' ? v : 'system'
}

const detectSystem = (): ResolvedMode =>
  window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'

export const ThemeProvider = ({ children }: { children: ReactNode }) => {
  const { lang } = useLanguage()
  const [mode, setModeState] = useState<ThemeMode>(readStoredMode)
  const [systemMode, setSystemMode] = useState<ResolvedMode>(detectSystem)

  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = (e: MediaQueryListEvent) =>
      setSystemMode(e.matches ? 'dark' : 'light')
    mq.addEventListener('change', handler)
    return () => mq.removeEventListener('change', handler)
  }, [])

  const resolvedMode: ResolvedMode = mode === 'system' ? systemMode : mode
  const tokens = useMemo(() => tokensFor(resolvedMode), [resolvedMode])

  useEffect(() => {
    document.documentElement.dataset.theme = resolvedMode
    applyCssVars(tokensToCssVars(tokens))
  }, [resolvedMode, tokens])

  const setMode = useCallback((m: ThemeMode) => {
    localStorage.setItem(STORAGE_KEY, m)
    setModeState(m)
  }, [])

  const antdConfig = useMemo(
    () => buildAntdTheme(resolvedMode, tokens),
    [resolvedMode, tokens],
  )
  const locale = lang === 'zh' ? zhCN : enUS

  return (
    <ThemeContext.Provider value={{ mode, resolvedMode, setMode, tokens }}>
      <ConfigProvider locale={locale} theme={antdConfig}>
        {children}
      </ConfigProvider>
    </ThemeContext.Provider>
  )
}
