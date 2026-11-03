import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: {
    saveConfig: vi.fn(),
    getState: vi.fn(),
  },
}))

import { api } from '../lib/api'
import Settings from './Settings'
import { useBeaconStore } from '../store/beacon'

const config = {
  version: 1,
  server: { host: '127.0.0.1', port: 6980 },
  mapping: {
    none: 'off',
    idle: 'calm',
    working: 'busy',
    waiting: 'attention',
    error: 'alert',
  },
  manual_override_default_secs: 60,
  shutdown: { turn_off_lights: true },
  mlight: { brightness: 50 },
  yh50: { buzzer_on_alert: false },
  devices: [],
  mock: { enabled: false },
}

const snapshot = {
  aggregate: 'none',
  effect: 'off',
  sessions: [],
  devices: [],
  service: { version: '0.1.0', pid: 1, port: 6980, uptime_secs: 1 },
  override: null,
  config,
}

describe('Settings', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useBeaconStore.setState({ snapshot, loading: false, error: null })
    api.saveConfig.mockResolvedValue(config)
    api.getState.mockResolvedValue(snapshot)
  })

  it('saves numeric brightness and mapping values', async () => {
    const user = userEvent.setup()
    render(<Settings />)

    expect(screen.queryByText('启用 mock 设备')).not.toBeInTheDocument()

    const brightness = screen.getByLabelText('灯光亮度')
    await user.clear(brightness)
    await user.type(brightness, '55')
    await waitFor(
      () => {
        expect(api.saveConfig).toHaveBeenCalled()
      },
      { timeout: 2500 },
    )
    const saved = api.saveConfig.mock.calls[0][0]
    expect(saved.mlight.brightness).toBe(55)
    expect(saved.mapping.error).toBe('alert')
    expect(await screen.findByText('已保存', {}, { timeout: 2500 })).toBeInTheDocument()
  })

  it('requires confirmation before enabling the alert buzzer', async () => {
    const user = userEvent.setup()
    render(<Settings />)

    const buzzer = screen.getByRole('switch', { name: '告警蜂鸣' })
    expect(buzzer).not.toBeChecked()

    await user.click(buzzer)
    expect(await screen.findByText('开启告警蜂鸣？')).toBeInTheDocument()
    expect(buzzer).not.toBeChecked()

    await user.click(screen.getByRole('button', { name: '确认开启' }))
    await waitFor(
      () => {
        expect(api.saveConfig).toHaveBeenCalled()
      },
      { timeout: 2500 },
    )

    const saved = api.saveConfig.mock.calls.at(-1)[0]
    expect(saved.yh50.buzzer_on_alert).toBe(true)
  })
})
