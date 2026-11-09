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

// jsdom 未实现指针捕获与滚动，Radix UI 的 Select/Dialog 会直接调用它们。
if (!window.Element.prototype.hasPointerCapture) {
  window.Element.prototype.hasPointerCapture = () => false
  window.Element.prototype.setPointerCapture = () => {}
  window.Element.prototype.releasePointerCapture = () => {}
}
if (!window.Element.prototype.scrollIntoView) {
  window.Element.prototype.scrollIntoView = () => {}
}
