import { translateNow } from '../i18n'

async function request(path, options = {}) {
  const response = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  })
  const text = await response.text()
  let data = null
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      data = null
    }
  }
  if (!response.ok) {
    throw new Error(data?.error || translateNow('api.httpError', { status: response.status }))
  }
  return data
}

export const api = {
  getState: () => request('/api/v1/state'),
  getAgents: () => request('/api/v1/agents'),
  getSessions: (limit) =>
    request(`/api/v1/sessions${limit ? `?limit=${limit}` : ''}`),
  getDevices: () => request('/api/v1/devices'),
  getConfig: () => request('/api/v1/config'),
  saveConfig: (config) =>
    request('/api/v1/config', { method: 'PUT', body: JSON.stringify(config) }),
  report: (event) =>
    request('/api/v1/events', { method: 'POST', body: JSON.stringify(event) }),
  clearSessions: () => request('/api/v1/sessions', { method: 'DELETE' }),
  scan: () => request('/api/v1/devices/scan', { method: 'POST' }),
  rebuildRouting: () => request('/api/v1/routing/rebuild', { method: 'POST' }),
  control: ({ effect, duration_secs, device }) =>
    request('/api/v1/control', {
      method: 'POST',
      body: JSON.stringify({ effect, duration_secs, device }),
    }),
  cancelControl: () => request('/api/v1/control/auto', { method: 'POST' }),
  getIntegrations: () => request('/api/v1/integrations'),
  getHooks: (limit) =>
    request(`/api/v1/hooks${limit ? `?limit=${limit}` : ''}`),
  saveIntegrationSettings: (settings) =>
    request('/api/v1/integrations/settings', {
      method: 'POST',
      body: JSON.stringify(settings),
    }),
  installIntegration: (id) =>
    request(`/api/v1/integrations/${id}/install`, { method: 'POST' }),
  upgradeIntegration: (id) =>
    request(`/api/v1/integrations/${id}/upgrade`, { method: 'POST' }),
  uninstallIntegration: (id) =>
    request(`/api/v1/integrations/${id}/uninstall`, { method: 'POST' }),
  installAllIntegrations: () =>
    request('/api/v1/integrations/install-all', { method: 'POST' }),
}
