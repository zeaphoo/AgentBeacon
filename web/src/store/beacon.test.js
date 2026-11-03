import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: {
    getState: vi.fn(),
    control: vi.fn(),
    cancelControl: vi.fn(),
    clearSessions: vi.fn(),
    scan: vi.fn(),
    saveConfig: vi.fn(),
  },
}))

import { api } from '../lib/api'
import { useBeaconStore } from './beacon'

const snapshot = {
  aggregate: 'working',
  effect: 'busy',
  sessions: [],
  devices: [],
  service: { version: '0.1.0', pid: 1, port: 6980, uptime_secs: 1 },
  config: { manual_override_default_secs: 60 },
}

describe('beacon store', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useBeaconStore.setState({
      snapshot: null,
      connected: false,
      loading: true,
      error: null,
    })
  })

  it('stores an SSE snapshot and clears loading', () => {
    useBeaconStore.getState().setSnapshot(snapshot)
    const state = useBeaconStore.getState()
    expect(state.snapshot).toEqual(snapshot)
    expect(state.loading).toBe(false)
    expect(state.error).toBeNull()
  })

  it('applies a manual override snapshot returned by the API', async () => {
    api.control.mockResolvedValue({ ...snapshot, effect: 'alert' })
    await useBeaconStore.getState().applyControl({ effect: 'alert', duration_secs: 30 })
    expect(api.control).toHaveBeenCalledWith({ effect: 'alert', duration_secs: 30 })
    expect(useBeaconStore.getState().snapshot.effect).toBe('alert')
  })

  it('surfaces load errors', async () => {
    api.getState.mockRejectedValue(new Error('boom'))
    await useBeaconStore.getState().load()
    expect(useBeaconStore.getState().error).toBe('boom')
    expect(useBeaconStore.getState().loading).toBe(false)
  })
})
