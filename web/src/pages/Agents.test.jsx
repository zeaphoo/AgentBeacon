import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../lib/api', () => ({
  api: {
    getState: vi.fn(),
    getIntegrations: vi.fn(),
    saveIntegrationSettings: vi.fn(),
    installIntegration: vi.fn(),
    upgradeIntegration: vi.fn(),
    uninstallIntegration: vi.fn(),
    installAllIntegrations: vi.fn(),
  },
}))

import { api } from '../lib/api'
import Agents from './Agents'
import { useBeaconStore } from '../store/beacon'

function entry(overrides) {
  return {
    id: 'pi',
    label: 'Pi',
    kind: 'plugin',
    detected: true,
    state: 'up-to-date',
    installed_version: '0.1.0-rc.1',
    bundled_version: '0.1.0-rc.1',
    dir: '/home/u/.pi/agent/extensions',
    path: '/home/u/.pi/agent/extensions/agent-beacon-hook-pi.js',
    error: null,
    ...overrides,
  }
}

const integrations = {
  auto_install: false,
  onboarded: true,
  detected: 3,
  installed: 1,
  pending: ['claude-code', 'codex'],
  entries: [
    entry({}),
    entry({
      id: 'claude-code',
      label: 'Claude Code',
      kind: 'hooks',
      state: 'not-installed',
      installed_version: null,
      path: '/home/u/.claude/settings.json',
    }),
    entry({
      id: 'codex',
      label: 'Codex CLI',
      kind: 'notify',
      state: 'upgrade-available',
      installed_version: '0.0.9',
      path: '/home/u/.codex/config.toml',
    }),
    entry({
      id: 'qwen',
      label: 'Qwen Code',
      kind: 'hooks',
      detected: false,
      state: 'not-installed',
      installed_version: null,
      path: '/home/u/.qwen/settings.json',
    }),
  ],
}

describe('Agents', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useBeaconStore.setState({
      snapshot: null,
      integrations: null,
      integrationsLoading: false,
      integrationsError: null,
    })
    api.getIntegrations.mockResolvedValue(integrations)
    api.getState.mockResolvedValue(null)
    api.saveIntegrationSettings.mockResolvedValue({ ...integrations, auto_install: true })
    api.installIntegration.mockResolvedValue({ id: 'claude-code', integrations })
    api.upgradeIntegration.mockResolvedValue({ id: 'codex', integrations })
    api.uninstallIntegration.mockResolvedValue({ id: 'pi', integrations })
    api.installAllIntegrations.mockResolvedValue({ results: [], integrations })
  })

  it('lists detected agents and marks the ones that are missing', async () => {
    render(<Agents />)

    expect(await screen.findByText('Claude Code')).toBeInTheDocument()
    expect(screen.getByText('检测到 3 个，已接入 1 个')).toBeInTheDocument()
    expect(screen.getAllByText('已检测到')).toHaveLength(3)
    expect(screen.getByText('未检测到')).toBeInTheDocument()
    expect(screen.getByText('版本 0.0.9')).toBeInTheDocument()
  })

  it('installs a single integration', async () => {
    const user = userEvent.setup()
    render(<Agents />)

    const [installButton] = await screen.findAllByRole('button', { name: '安装' })
    await user.click(installButton)

    await waitFor(() => {
      expect(api.installIntegration).toHaveBeenCalledWith('claude-code')
    })
  })

  it('toggles automatic installation', async () => {
    const user = userEvent.setup()
    render(<Agents />)

    const toggle = await screen.findByRole('switch', { name: '自动安装' })
    expect(toggle).not.toBeChecked()
    await user.click(toggle)

    await waitFor(() => {
      expect(api.saveIntegrationSettings).toHaveBeenCalledWith({ auto_install: true })
    })
  })

  it('installs every pending integration at once', async () => {
    const user = userEvent.setup()
    render(<Agents />)

    const button = await screen.findByRole('button', { name: '安装全部' })
    expect(button).toBeEnabled()
    await user.click(button)

    await waitFor(() => {
      expect(api.installAllIntegrations).toHaveBeenCalled()
    })
  })

  it('surfaces action failures', async () => {
    const user = userEvent.setup()
    api.installIntegration.mockRejectedValue(new Error('permission denied'))
    render(<Agents />)

    const [installButton] = await screen.findAllByRole('button', { name: '安装' })
    await user.click(installButton)

    expect(await screen.findByRole('alert')).toHaveTextContent(
      '操作失败：permission denied',
    )
  })
})