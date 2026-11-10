import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: {
    saveConfig: vi.fn(),
    getState: vi.fn(),
    getIntegrations: vi.fn(),
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
    useBeaconStore.setState({
      snapshot,
      loading: false,
      error: null,
      integrations: null,
    })
    api.saveConfig.mockResolvedValue(config)
    api.getState.mockResolvedValue(snapshot)
    api.getIntegrations.mockResolvedValue({
      entries: [],
      pending: [],
      auto_install: false,
      onboarded: false,
      detected: 0,
      installed: 0,
    })
  })

  it('saves numeric brightness and mapping values', async () => {
    const user = userEvent.setup()
    render(<Settings />)

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

  it('keeps manual lights and lets the user delete stale ones', async () => {
    const routingSnapshot = {
      ...snapshot,
      sessions: [
        {
          agent: 'claude',
          session: 'claude:1',
          state: 'working',
          message: null,
          updated_at_ms: Date.now(),
          expires_at_ms: Date.now() + 60_000,
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
          last_seen_ms: null,
        },
      ],
      routing: [
        {
          device: 'serial:A',
          label: '左灯',
          assignment: 'manual',
          agents: [],
          sessions: [],
          aggregate: 'none',
          auto_effect: 'off',
          effect: 'off',
          overridden: false,
          connected: true,
          last_seen_ms: null,
        },
      ],
      unassigned_agents: ['claude'],
      config: {
        ...config,
        devices: [
          {
            id: 'serial:A',
            port: '/dev/A',
            serial: null,
            profile: 'mlight-3led',
            label: '左灯',
          },
        ],
        routing: {
          strategy: 'unified',
          manual: { 'serial:GONE': ['pi'] },
        },
      },
    }
    useBeaconStore.setState({ snapshot: routingSnapshot, loading: false, error: null })
    api.getState.mockResolvedValue(routingSnapshot)

    const user = userEvent.setup()
    render(<Settings />)

    await user.click(screen.getByRole('button', { name: /按 Agent 手动分配/ }))
    expect(screen.getAllByText('serial:GONE').length).toBeGreaterThan(0)
    expect(screen.getByText('pi')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '删除这台灯的配置' }))
    await waitFor(
      () => {
        const saved = api.saveConfig.mock.calls.at(-1)?.[0]
        expect(saved?.routing.manual?.['serial:GONE']).toBeUndefined()
        expect(saved?.routing.strategy).toBe('manual_agent')
      },
      { timeout: 2500 },
    )

    await user.click(screen.getByRole('combobox', { name: '添加 Agent · 左灯' }))
    await user.click(await screen.findByRole('option', { name: 'claude' }))
    await waitFor(
      () => {
        const saved = api.saveConfig.mock.calls.at(-1)?.[0]
        expect(saved?.routing.manual?.['serial:A']).toEqual(['claude'])
      },
      { timeout: 2500 },
    )
  })

  it('offers detected agents even before they report a session', async () => {
    useBeaconStore.setState({
      snapshot: {
        ...snapshot,
        devices: [
          {
            id: 'serial:A',
            port: '/dev/A',
            profile: 'mlight-3led',
            label: '左灯',
            connected: true,
            effect: 'off',
            last_seen_ms: null,
          },
        ],
        routing: [
          {
            device: 'serial:A',
            label: '左灯',
            assignment: 'manual',
            agents: [],
            sessions: [],
            aggregate: 'none',
            auto_effect: 'off',
            effect: 'off',
            overridden: false,
            connected: true,
            last_seen_ms: null,
          },
        ],
        config: {
          ...config,
          devices: [
            {
              id: 'serial:A',
              port: '/dev/A',
              serial: null,
              profile: 'mlight-3led',
              label: '左灯',
            },
          ],
          routing: { strategy: 'manual_agent', manual: {} },
        },
      },
      integrations: {
        entries: [
          { id: 'claude-code', label: 'Claude Code', detected: true },
          { id: 'codex', label: 'Codex CLI', detected: true },
          { id: 'gemini', label: 'Gemini CLI', detected: false },
        ],
      },
      loading: false,
    })

    const user = userEvent.setup()
    render(<Settings />)

    await user.click(screen.getByRole('combobox', { name: '添加 Agent · 左灯' }))
    expect(await screen.findByRole('option', { name: 'claude-code' })).toBeInTheDocument()
    expect(screen.getByRole('option', { name: 'codex' })).toBeInTheDocument()
    expect(screen.queryByRole('option', { name: 'gemini' })).not.toBeInTheDocument()
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
