/**
 * 控制台里“每台灯”的统一视图。
 *
 * 只使用运行时发现的设备：
 * 配置里存在、但当前没有插上的绑定不是“设备”，不出现在首页/灯具页。
 *
 * routing 只用来补充每台灯的分配、会话和灯效信息，不扩大设备集合。
 */
export function resolveLights(snapshot) {
  const devices = snapshot?.devices ?? []
  const routes = new Map(
    (snapshot?.routing ?? []).map((route) => [route.device, route]),
  )
  const fallbackAggregate = snapshot?.aggregate ?? 'none'

  return devices.map((device) => {
    const route = routes.get(device.id)
    return {
      ...route,
      device: device.id,
      id: device.id,
      port: device.port,
      profile: device.profile,
      label: route?.label ?? device.label,
      connected: route?.connected ?? Boolean(device.connected),
      effect: route?.effect ?? device.effect,
      last_error: device.last_error ?? null,
      last_seen_ms: route?.last_seen_ms ?? device.last_seen_ms ?? null,
      assignment: route?.assignment ?? 'mirror',
      agents: route?.agents ?? [],
      sessions: route?.sessions ?? [],
      aggregate: route?.aggregate ?? fallbackAggregate,
      auto_effect: route?.auto_effect ?? device.effect,
      overridden: route?.overridden ?? false,
      capabilities: device.capabilities,
    }
  })
}
