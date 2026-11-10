import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: { getState: vi.fn().mockResolvedValue(null) },
}))

vi.mock('../hooks/useBeaconStream', () => ({
  useBeaconStream: () => {},
}))

import Layout from './Layout'
import { useBeaconStore } from '../store/beacon'
import { usePreferencesStore } from '../store/preferences'

describe('Layout header controls', () => {
  beforeEach(() => {
    useBeaconStore.setState({ snapshot: null, connected: false, loading: false, error: null })
    usePreferencesStore.setState({ language: 'zh', theme: 'dark' })
  })

  it('renders compact icon triggers that open a menu', () => {
    render(
      <MemoryRouter>
        <Layout />
      </MemoryRouter>,
    )

    const language = screen.getByRole('combobox', { name: '语言' })
    const theme = screen.getByRole('combobox', { name: '主题' })

    // 触发按钮只显示图标：没有文字标签。
    expect(language).toHaveTextContent('')
    expect(theme).toHaveTextContent('')
    expect(language.querySelectorAll('svg')).toHaveLength(2)
  })
})
