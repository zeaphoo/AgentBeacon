import { useEffect } from 'react'
import {
  PREFS_STORAGE_KEY,
  resolveTheme,
  usePreferencesStore,
} from '../store/preferences'

export default function PreferencesSync() {
  const language = usePreferencesStore((state) => state.language)
  const theme = usePreferencesStore((state) => state.theme)
  const systemTheme = usePreferencesStore((state) => state.systemTheme)
  const syncSystemTheme = usePreferencesStore((state) => state.syncSystemTheme)
  const resolvedTheme = resolveTheme(theme, systemTheme)

  useEffect(() => {
    const root = document.documentElement
    root.dataset.theme = resolvedTheme
    root.style.colorScheme = resolvedTheme
    const meta = document.querySelector('meta[name="theme-color"]')
    if (meta) {
      meta.setAttribute(
        'content',
        resolvedTheme === 'light' ? '#f7faf9' : '#0b0f14',
      )
    }
  }, [resolvedTheme])

  useEffect(() => {
    document.documentElement.lang = language === 'zh' ? 'zh-CN' : 'en'
  }, [language])

  useEffect(() => {
    try {
      window.localStorage.setItem(
        PREFS_STORAGE_KEY,
        JSON.stringify({ language, theme }),
      )
    } catch {
      // Preferences are best-effort; storage may be unavailable in private mode.
    }
  }, [language, theme])

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return

    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const handleChange = () => syncSystemTheme()
    if (typeof media.addEventListener === 'function') {
      media.addEventListener('change', handleChange)
      return () => media.removeEventListener('change', handleChange)
    }
    media.addListener(handleChange)
    return () => media.removeListener(handleChange)
  }, [syncSystemTheme])

  return null
}
