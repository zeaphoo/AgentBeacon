import { create } from 'zustand'

export const PREFS_STORAGE_KEY = 'agent-beacon.prefs'

export function detectSystemLanguage() {
  if (typeof navigator === 'undefined') return 'en'
  const tag = (navigator.languages && navigator.languages[0]) || navigator.language || 'en'
  return String(tag).toLowerCase().startsWith('zh') ? 'zh' : 'en'
}

export function detectSystemTheme() {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return 'dark'
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

function readStoredPrefs() {
  if (typeof window === 'undefined') return {}
  try {
    return JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) || '{}')
  } catch {
    return {}
  }
}

const stored = readStoredPrefs()

export const usePreferencesStore = create((set) => ({
  language:
    stored.language === 'zh' || stored.language === 'en'
      ? stored.language
      : detectSystemLanguage(),
  theme:
    stored.theme === 'system' || stored.theme === 'light' || stored.theme === 'dark'
      ? stored.theme
      : 'system',
  systemTheme: detectSystemTheme(),

  setLanguage(language) {
    set({ language })
  },

  setTheme(theme) {
    set({ theme })
  },

  syncSystemTheme() {
    set({ systemTheme: detectSystemTheme() })
  },
}))

export function resolveTheme(preference, systemTheme) {
  return preference === 'system' ? systemTheme : preference
}
