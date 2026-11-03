import { create } from 'zustand'
import { api } from '../lib/api'

export const useBeaconStore = create((set, get) => ({
  snapshot: null,
  connected: false,
  loading: true,
  error: null,

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

  async scanDevices() {
    await api.scan()
    await new Promise((resolve) => setTimeout(resolve, 900))
    const snapshot = await api.getState()
    set({ snapshot })
    return snapshot
  },

  getSnapshot() {
    return get().snapshot
  },
}))
