import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: {
    control: vi.fn(),
    cancelControl: vi.fn(),
    saveConfig: vi.fn(),
    getState: vi.fn(),
  },
}))

import { api } from '../lib/api'
import Control from './Control'
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
  aggregate: 'working',
  effect: 'busy',
  sessions: [],
  devices: [
    { id: 'mock:0', port: 'mock', profile: 'mock', connected: true, effect: 'busy' },
    {
      id: 'ml:0',
      port: '/dev/ttyUSB0',
      profile: 'mlight-single',
      connected: true,
      effect: 'busy',
      last_error: null,
    },
  ],
  service: { version: '0.1.0', pid: 1, port: 6980, uptime_secs: 1 },
  override: null,
  config,
}

describe('Control', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useBeaconStore.setState({ snapshot, loading: false, error: null })
    api.control.mockResolvedValue({
      ...snapshot,
      override: { effect: 'attention', expires_at_ms: Date.now() + 30_000, device: null },
    })
    api.saveConfig.mockResolvedValue(config)
    api.getState.mockResolvedValue(snapshot)
  })

  it('applies the selected effect with duration and device', async () => {
    const user = userEvent.setup()
    render(<Control />)

    await user.click(screen.getByText('需要关注').closest('button'))
    const duration = screen.getByLabelText('持续时长（秒）')
    await user.clear(duration)
    await user.type(duration, '30')
    await user.click(screen.getByRole('button', { name: /应用灯效/ }))

    await waitFor(() => {
      expect(api.control).toHaveBeenCalledWith({
        effect: 'attention',
        duration_secs: 30,
        device: null,
      })
    })
    expect(screen.getByText(/已应用 需要关注/)).toBeInTheDocument()
  })

  it('saves the profile binding chosen in the device list', async () => {
    const user = userEvent.setup()
    render(<Control />)

    expect(screen.getByText('/dev/ttyUSB0')).toBeInTheDocument()
    await user.click(screen.getByRole('combobox', { name: '设备型号 · /dev/ttyUSB0' }))
    await user.click(await screen.findByRole('option', { name: 'MLight 三灯珠' }))

    await waitFor(() => {
      expect(api.saveConfig).toHaveBeenCalled()
    })
    const saved = api.saveConfig.mock.calls.at(-1)[0]
    expect(saved.devices).toEqual([
      { port: '/dev/ttyUSB0', serial: null, profile: 'mlight-3led' },
    ])
    expect(await screen.findByText('已保存')).toBeInTheDocument()
  })

  it('opens the supported models dialog instead of listing them inline', async () => {
    const user = userEvent.setup()
    render(<Control />)

    expect(screen.queryByText('支持的设备型号')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /支持的型号/ }))

    const dialog = await screen.findByRole('dialog')
    expect(dialog).toHaveTextContent('支持的设备型号')
    expect(dialog).toHaveTextContent('三色 LED 与蜂鸣器，支持告警蜂鸣')
  })
})
