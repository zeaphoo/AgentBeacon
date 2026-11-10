import { create } from 'zustand'
import { api } from '../lib/api'

export const useBeaconStore = create((set, get) => ({
  snapshot: null,
  connected: false,
  loading: true,
  error: null,
  // Agent 集成列表与设置，独立于状态快照（变化频率低）。
  integrations: null,
  integrationsLoading: false,
  integrationsError: null,

  setSnapshot(snapshot) {
    set({ snapshot, loading: false, error: null })
  },

  setConnected(connected) {
    set({ connected })
  },

  setError(error) {
    set({ error, loading: false })
  },

  async load() {
    try {
      const snapshot = await api.getState()
      set({ snapshot, loading: false, error: null })
    } catch (error) {
      set({ error: error.message, loading: false })
    }
  },

  async clearSessions() {
    await api.clearSessions()
    const snapshot = await api.getState()
    set({ snapshot, error: null })
  },

  async applyControl(payload) {
    const snapshot = await api.control(payload)
    set({ snapshot, error: null })
    return snapshot
  },

  async cancelControl() {
    const snapshot = await api.cancelControl()
    set({ snapshot, error: null })
    return snapshot
  },

  async saveConfig(config) {
    await api.saveConfig(config)
    const snapshot = await api.getState()
    set({ snapshot, error: null })
    return snapshot
  },

  async rebuildRouting() {
    const snapshot = await api.rebuildRouting()
    set({ snapshot, error: null })
    return snapshot
  },

  async scanDevices() {
    await api.scan()
    await new Promise((resolve) => setTimeout(resolve, 900))
    const snapshot = await api.getState()
    set({ snapshot })
    return snapshot
  },

  async loadIntegrations() {
    set({ integrationsLoading: true })
    try {
      const integrations = await api.getIntegrations()
      set({ integrations, integrationsLoading: false, integrationsError: null })
      return integrations
    } catch (error) {
      set({ integrationsLoading: false, integrationsError: error.message })
      return null
    }
  },

  async saveIntegrationSettings(settings) {
    const integrations = await api.saveIntegrationSettings(settings)
    set({ integrations, integrationsError: null })
    // auto_install 存在配置里，同步刷新快照避免设置页看到旧值。
    get().load()
    return integrations
  },

  async installIntegration(id) {
    const result = await api.installIntegration(id)
    set({ integrations: result.integrations, integrationsError: null })
    return result
  },

  async upgradeIntegration(id) {
    const result = await api.upgradeIntegration(id)
    set({ integrations: result.integrations, integrationsError: null })
    return result
  },

  async uninstallIntegration(id) {
    const result = await api.uninstallIntegration(id)
    set({ integrations: result.integrations, integrationsError: null })
    return result
  },

  async installAllIntegrations() {
    const result = await api.installAllIntegrations()
    set({ integrations: result.integrations, integrationsError: null })
    get().load()
    return result
  },

  getSnapshot() {
    return get().snapshot
  },
}))
