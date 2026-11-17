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

export const DEFAULT_OVERRIDE_DURATION_SECS = 60
export const MAX_OVERRIDE_DURATION_SECS = 3600

function validOverrideDuration(value) {
  const seconds = Number(value)
  return Number.isFinite(seconds) &&
    seconds >= 1 &&
    seconds <= MAX_OVERRIDE_DURATION_SECS
    ? Math.round(seconds)
    : null
}

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
  // 控制台记住上次手动覆盖用的时长，替代原来的全局默认时长配置。
  overrideDurationSecs:
    validOverrideDuration(stored.overrideDurationSecs) ?? DEFAULT_OVERRIDE_DURATION_SECS,

  setLanguage(language) {
    set({ language })
  },

  setTheme(theme) {
    set({ theme })
  },

  setOverrideDurationSecs(seconds) {
    const value = validOverrideDuration(seconds)
    if (value != null) set({ overrideDurationSecs: value })
  },

  syncSystemTheme() {
    set({ systemTheme: detectSystemTheme() })
  },
}))

export function resolveTheme(preference, systemTheme) {
  return preference === 'system' ? systemTheme : preference
}
