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

/// 条件请求：带上上次的 ETag，内容没变时服务端回 304（空 body）。
///
/// 控制台每秒轮询就靠它，避免每次传输整份快照。
async function getStateIfChanged(etag) {
  const headers = {}
  if (etag) headers['If-None-Match'] = etag
  const response = await fetch('/api/v1/state', { headers })
  if (response.status === 304) return { notModified: true, etag }
  if (!response.ok) {
    throw new Error(translateNow('api.httpError', { status: response.status }))
  }
  return {
    notModified: false,
    etag: response.headers.get('ETag'),
    snapshot: await response.json(),
  }
}

export const api = {
  getState: () => request('/api/v1/state'),
  getStateIfChanged,
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
