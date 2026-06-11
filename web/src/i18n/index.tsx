import React, { createContext, useContext, useState, useCallback } from 'react'
import en from './en'
import zh from './zh'

type Lang = 'en' | 'zh'

type Translations = typeof en

const translations: Record<Lang, Translations> = { en, zh }

interface LanguageContextValue {
  lang: Lang
  setLang: (lang: Lang) => void
}

const LanguageContext = createContext<LanguageContextValue>({
  lang: 'en',
  setLang: () => {},
})

const STORAGE_KEY = 'edgion-lang'

export const I18nProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [lang, setLangState] = useState<Lang>(() => {
    const stored = localStorage.getItem(STORAGE_KEY)
    return stored === 'zh' ? 'zh' : 'en'
  })

  const setLang = useCallback((newLang: Lang) => {
    localStorage.setItem(STORAGE_KEY, newLang)
    setLangState(newLang)
  }, [])

  return (
    <LanguageContext.Provider value={{ lang, setLang }}>
      {children}
    </LanguageContext.Provider>
  )
}

export const useLanguage = () => useContext(LanguageContext)

export const useT = () => {
  const { lang } = useLanguage()
  const t = useCallback(
    (key: string, params?: Record<string, string | number>): string => {
      const dict = translations[lang] as Record<string, string>
      let str = dict[key] ?? key
      if (params) {
        Object.entries(params).forEach(([k, v]) => {
          str = str.replace(new RegExp(`\\{${k}\\}`, 'g'), String(v))
        })
      }
      return str
    },
    [lang],
  )
  return t
}
