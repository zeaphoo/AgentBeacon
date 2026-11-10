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
      id: 'ml:0',
      port: '/dev/ttyUSB0',
      profile: 'mlight-3led',
      connected: true,
      effect: 'attention',
      last_error: null,
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
    const { container } = render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    )

    expect(screen.getAllByText('等待确认').length).toBeGreaterThan(0)
    const sessionRow = screen.getByText('claude-code').closest('li')
    expect(sessionRow.firstElementChild).toHaveTextContent('等待确认')
    expect(sessionRow.children[1].getAttribute('data-agent-logo')).toBe('claude-code')
    expect(container.querySelector('[data-agent-logo="claude-code"]')).toBeTruthy()
    expect(screen.getByText('MLight 三灯珠')).toBeInTheDocument()
    expect(screen.getByText('已连接')).toBeInTheDocument()
    expect(screen.getByText('串口')).toBeInTheDocument()
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

  it('shows a status card per light', () => {
    useBeaconStore.setState({
      snapshot: {
        ...snapshot,
        aggregate: 'error',
        effect: 'alert',
        sessions: [],
        config: {
          ...snapshot.config,
          routing: { strategy: 'auto_agent' },
        },
        routing: [
          {
            device: 'serial:A',
            label: '左灯',
            assignment: 'auto',
            agents: ['claude'],
            sessions: [],
            aggregate: 'working',
            auto_effect: 'busy',
            effect: 'busy',
            overridden: false,
            connected: true,
            last_seen_ms: null,
          },
          {
            device: 'serial:B',
            label: '右灯',
            assignment: 'auto',
            agents: ['codex'],
            sessions: [],
            aggregate: 'error',
            auto_effect: 'alert',
            effect: 'alert',
            overridden: false,
            connected: true,
            last_seen_ms: null,
          },
        ],
        devices: [
          {
            id: 'serial:A',
            port: '/dev/A',
            profile: 'mlight-3led',
            label: '左灯',
            connected: true,
            effect: 'busy',
            last_error: null,
          },
          {
            id: 'serial:B',
            port: '/dev/B',
            profile: 'agentcore-light',
            label: '右灯',
            connected: true,
            effect: 'alert',
            last_error: null,
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

    expect(screen.getByTestId('device-serial:A')).toHaveTextContent('左灯')
    expect(screen.getByTestId('device-serial:A')).toHaveTextContent('分配 Agent：claude')
    expect(screen.getByTestId('device-serial:B')).toHaveTextContent('右灯')
    expect(screen.getByTestId('device-serial:B')).toHaveTextContent('分配 Agent：codex')
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
