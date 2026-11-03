import '@testing-library/jest-dom/vitest'

// Keep UI language/theme deterministic in tests. The preferences store reads
// localStorage at module initialization, so this must run before test imports.
window.localStorage.setItem(
  'agent-beacon.prefs',
  JSON.stringify({ language: 'zh', theme: 'dark' }),
)
Object.defineProperty(window.navigator, 'language', {
  value: 'zh-CN',
  configurable: true,
})
Object.defineProperty(window.navigator, 'languages', {
  value: ['zh-CN'],
  configurable: true,
})
