import { describe, expect, it } from 'vitest'
import { resolveLights } from './lights'

describe('resolveLights', () => {
  it('only includes runtime devices and excludes configured-but-offline bindings', () => {
    const snapshot = {
      aggregate: 'idle',
      devices: [
        { id: 'serial:A', port: '/dev/A', profile: 'mlight-3led', connected: true, effect: 'calm' },
      ],
      routing: [
        { device: 'serial:A', connected: true, effect: 'calm', agents: ['pi'] },
        { device: 'serial:B', connected: false, effect: 'off', agents: [] },
      ],
    }

    const lights = resolveLights(snapshot)
    expect(lights).toHaveLength(1)
    expect(lights[0].device).toBe('serial:A')
    expect(lights[0].agents).toEqual(['pi'])
  })

  it('passes hardware telemetry through to the light view', () => {
    const snapshot = {
      aggregate: 'idle',
      devices: [
        {
          id: 'ble:24:26:4c:c3:fd:10',
          port: 'ble:24:26:4c:c3:fd:10',
          profile: 'agentcore-halo',
          connected: true,
          effect: 'calm',
          name: 'AgentCore-Halo-BLE',
          firmware: 'v0.5.12',
          battery: { percent: 82, usb_online: true, millivolts: 387, updated_at_ms: 1 },
        },
      ],
      routing: [{ device: 'ble:24:26:4c:c3:fd:10', connected: true, effect: 'calm' }],
    }

    const [light] = resolveLights(snapshot)
    expect(light.name).toBe('AgentCore-Halo-BLE')
    expect(light.firmware).toBe('v0.5.12')
    expect(light.battery.percent).toBe(82)
    expect(light.profile).toBe('agentcore-halo')
  })

  it('keeps runtime device fields when routing is unavailable', () => {
    const snapshot = {
      aggregate: 'working',
      devices: [
        { id: 'serial:A', port: '/dev/A', profile: 'mlight-3led', connected: true, effect: 'busy' },
      ],
    }

    const lights = resolveLights(snapshot)
    expect(lights).toHaveLength(1)
    expect(lights[0]).toMatchObject({
      device: 'serial:A',
      port: '/dev/A',
      profile: 'mlight-3led',
      connected: true,
      effect: 'busy',
      aggregate: 'working',
    })
  })
})
