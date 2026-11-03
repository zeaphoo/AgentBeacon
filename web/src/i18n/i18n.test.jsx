import { act, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { useI18n } from './index'
import { messages } from './messages'
import { usePreferencesStore } from '../store/preferences'

function Probe() {
  const { t, language } = useI18n()
  return (
    <div>
      {language}:{t('nav.dashboard')}
    </div>
  )
}

describe('i18n', () => {
  it('uses the selected language immediately', () => {
    usePreferencesStore.setState({ language: 'zh' })
    render(<Probe />)
    expect(screen.getByText('zh:仪表盘')).toBeInTheDocument()

    act(() => usePreferencesStore.getState().setLanguage('en'))
    expect(screen.getByText('en:Dashboard')).toBeInTheDocument()
  })

  it('keeps zh and en message keys in sync', () => {
    expect(Object.keys(messages.en).sort()).toEqual(Object.keys(messages.zh).sort())
  })
})
