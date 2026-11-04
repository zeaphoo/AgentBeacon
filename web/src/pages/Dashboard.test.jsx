import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it } from 'vitest'
import Dashboard from './Dashboard'
import { useBeaconStore } from '../store/beacon'

const snapshot = {
  aggregate: 'waiting',
  effect: 'attention',
  sessions: [
    {
      agent: 'claude-code',
      session: 'claude-code:default',
      state: 'waiting',
      message: '等待确认',
      updated_at_ms: Date.now() - 1000,
      expires_at_ms: Date.now() + 900_000,
    },
  ],
  devices: [
    {
      id: 'mock:0',
      port: 'mock',
      profile: 'mock',
      connected: true,
      effect: 'attention',
      last_error: null,
      mock_commands: ['4D 4F 43 4B'],
    },
  ],
  service: { version: '0.1.0', pid: 42, port: 6980, uptime_secs: 12 },
  override: null,
  config: { manual_override_default_secs: 60 },
}

describe('Dashboard', () => {
  beforeEach(() => {
    useBeaconStore.setState({
      snapshot,
      connected: true,
      loading: false,
      error: null,
    })
  })

  it('renders aggregate, sessions and devices', () => {
    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    expect(screen.getAllByText('等待确认').length).toBeGreaterThan(0)
    expect(screen.getByText('claude-code')).toBeInTheDocument()
    expect(screen.getByText('Mock 设备')).toBeInTheDocument()
    expect(screen.getByText('已连接')).toBeInTheDocument()
    expect(screen.queryByText(/PID/)).not.toBeInTheDocument()
  })

  it('shows capabilities for devices that support them', () => {
    useBeaconStore.setState({
      snapshot: {
        ...snapshot,
        devices: [
          {
            ...snapshot.devices[0],
            id: 'ml:0',
            port: '/dev/ttyUSB0',
            profile: 'mlight-3led',
            capabilities: { brightness: true, buzzer: false },
          },
        ],
      },
      loading: false,
    })
    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    expect(screen.getByText('亮度可调')).toBeInTheDocument()
    expect(screen.queryByText('蜂鸣器')).not.toBeInTheDocument()
  })

  it('shows an empty state when there are no sessions', () => {
    useBeaconStore.setState({
      snapshot: { ...snapshot, sessions: [], devices: [] },
      loading: false,
    })
    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )
    expect(screen.getByText('还没有 agent 上报状态')).toBeInTheDocument()
    expect(screen.getByText('未发现设备')).toBeInTheDocument()
  })
})
