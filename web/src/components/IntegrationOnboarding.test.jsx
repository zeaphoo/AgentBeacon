import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: {
    getState: vi.fn(),
    getIntegrations: vi.fn(),
    saveIntegrationSettings: vi.fn(),
    installAllIntegrations: vi.fn(),
  },
}))

import { api } from '../lib/api'
import IntegrationOnboarding from './IntegrationOnboarding'
import { useBeaconStore } from '../store/beacon'

function payload(overrides) {
  return {
    auto_install: false,
    onboarded: false,
    detected: 2,
    installed: 0,
    pending: ['claude-code', 'codex'],
    entries: [
      {
        id: 'claude-code',
        label: 'Claude Code',
        kind: 'hooks',
        detected: true,
        state: 'not-installed',
        installed_version: null,
        bundled_version: '0.1.0-rc.1',
        dir: '/home/u/.claude',
        path: '/home/u/.claude/settings.json',
        error: null,
      },
      {
        id: 'codex',
        label: 'Codex CLI',
        kind: 'notify',
        detected: true,
        state: 'not-installed',
        installed_version: null,
        bundled_version: '0.1.0-rc.1',
        dir: '/home/u/.codex',
        path: '/home/u/.codex/config.toml',
        error: null,
      },
    ],
    ...overrides,
  }
}

describe('IntegrationOnboarding', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useBeaconStore.setState({
      snapshot: null,
      integrations: null,
      integrationsLoading: false,
      integrationsError: null,
    })
    api.getIntegrations.mockResolvedValue(payload())
    api.saveIntegrationSettings.mockResolvedValue(payload({ onboarded: true }))
    api.installAllIntegrations.mockResolvedValue({ results: [], integrations: payload({ onboarded: true }) })
  })

  it('prompts once when agents are detected but not connected', async () => {
    render(<IntegrationOnboarding />)

    expect(await screen.findByText('接入你的 code agent')).toBeInTheDocument()
    expect(
      screen.getByText(/检测到 2 个已安装但尚未接入的 agent/),
    ).toBeInTheDocument()
    expect(screen.getByText(/Claude Code、Codex CLI/)).toBeInTheDocument()
  })

  it('stays hidden after onboarding', async () => {
    api.getIntegrations.mockResolvedValue(payload({ onboarded: true }))
    render(<IntegrationOnboarding />)

    await waitFor(() => {
      expect(api.getIntegrations).toHaveBeenCalled()
    })
    expect(screen.queryByText('接入你的 code agent')).not.toBeInTheDocument()
  })

  it('stays hidden when nothing is pending', async () => {
    api.getIntegrations.mockResolvedValue(payload({ pending: [] }))
    render(<IntegrationOnboarding />)

    await waitFor(() => {
      expect(api.getIntegrations).toHaveBeenCalled()
    })
    expect(screen.queryByText('接入你的 code agent')).not.toBeInTheDocument()
  })

  it('connects everything and enables automatic installation', async () => {
    const user = userEvent.setup()
    render(<IntegrationOnboarding />)

    await screen.findByText('接入你的 code agent')
    await user.click(screen.getByRole('switch', { name: '以后为检测到的 agent 自动安装' }))
    await user.click(screen.getByRole('button', { name: '全部接入' }))

    await waitFor(() => {
      expect(api.saveIntegrationSettings).toHaveBeenCalledWith({ auto_install: true })
      expect(api.installAllIntegrations).toHaveBeenCalled()
    })
    await waitFor(() => {
      expect(screen.queryByText('接入你的 code agent')).not.toBeInTheDocument()
    })
  })

  it('marks onboarding done when skipped', async () => {
    const user = userEvent.setup()
    render(<IntegrationOnboarding />)

    await screen.findByText('接入你的 code agent')
    await user.click(screen.getByRole('button', { name: '稍后再说' }))

    await waitFor(() => {
      expect(api.saveIntegrationSettings).toHaveBeenCalledWith({
        auto_install: false,
        onboarded: true,
      })
    })
    expect(api.installAllIntegrations).not.toHaveBeenCalled()
  })
})