import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: {
    control: vi.fn(),
    cancelControl: vi.fn(),
  },
}))

import { api } from '../lib/api'
import Control from './Control'
import { useBeaconStore } from '../store/beacon'

const snapshot = {
  aggregate: 'working',
  effect: 'busy',
  sessions: [],
  devices: [{ id: 'mock:0', port: 'mock', profile: 'mock', connected: true, effect: 'busy' }],
  service: { version: '0.1.0', pid: 1, port: 6980, uptime_secs: 1 },
  override: null,
  config: { manual_override_default_secs: 60 },
}

describe('Control', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useBeaconStore.setState({ snapshot, loading: false, error: null })
    api.control.mockResolvedValue({
      ...snapshot,
      override: { effect: 'attention', expires_at_ms: Date.now() + 30_000, device: null },
    })
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
})
