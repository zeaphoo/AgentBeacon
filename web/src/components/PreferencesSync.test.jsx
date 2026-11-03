import { act, render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import PreferencesSync from './PreferencesSync'
import { usePreferencesStore } from '../store/preferences'

describe('PreferencesSync', () => {
  it('resolves the system theme and applies language to the document', () => {
    act(() =>
      usePreferencesStore.setState({
        language: 'en',
        theme: 'system',
        systemTheme: 'light',
      }),
    )
    render(<PreferencesSync />)

    expect(document.documentElement.dataset.theme).toBe('light')
    expect(document.documentElement.lang).toBe('en')

    act(() => usePreferencesStore.setState({ systemTheme: 'dark' }))
    expect(document.documentElement.dataset.theme).toBe('dark')

    act(() =>
      usePreferencesStore.setState({ language: 'zh', theme: 'light' }),
    )
    expect(document.documentElement.dataset.theme).toBe('light')
    expect(document.documentElement.lang).toBe('zh-CN')
  })
})
