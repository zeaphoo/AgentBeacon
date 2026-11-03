import { useCallback } from 'react'
import { usePreferencesStore } from '../store/preferences'
import { messages } from './messages'

export function translate(language, key, vars) {
  const dictionary = messages[language] ?? messages.zh
  let text = dictionary[key] ?? messages.zh[key] ?? key
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      text = text.split(`{${name}}`).join(String(value))
    }
  }
  return text
}

export function currentLanguage() {
  return usePreferencesStore.getState().language
}

export function translateNow(key, vars) {
  return translate(currentLanguage(), key, vars)
}

export function useI18n() {
  const language = usePreferencesStore((state) => state.language)
  const t = useCallback((key, vars) => translate(language, key, vars), [language])

  return { t, language }
}
