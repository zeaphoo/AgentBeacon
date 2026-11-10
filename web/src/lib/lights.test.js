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
