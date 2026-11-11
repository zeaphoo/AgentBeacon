import { describe, expect, it } from 'vitest'
import {
  connectionLabel,
  formatCountdown,
  formatDateTime,
  formatUptime,
  stateTone,
} from './labels'

const t = (key, vars) => (vars ? `${key.replace('time.', '')}:${vars.n}` : key)

describe('labels', () => {
  it('humanizes uptime with the largest useful units', () => {
    expect(formatUptime(59, t)).toBe('seconds:59')
    expect(formatUptime(60, t)).toBe('minutes:1')
    expect(formatUptime(7063, t)).toBe('hours:1 minutes:57')
    expect(formatUptime(90061, t)).toBe('days:1 hours:1')
  })


  it('labels connection types for BLE, SPP, serial and USB links', () => {
    const key = (name) => name
    expect(connectionLabel('ble:c3:a4:49:2c:6a', key)).toBe('connection.bluetooth')
    expect(connectionLabel('spp:AA:BB:CC:DD:EE:FF', key)).toBe('connection.bluetoothSpp')
    expect(connectionLabel('/dev/ttyACM0', key)).toBe('connection.serial')
    expect(connectionLabel('COM3', key)).toBe('connection.serial')
    expect(connectionLabel('serial:5055207940A3461C', key)).toBe('connection.serial')
    expect(connectionLabel('usb:1A86:7523', key)).toBe('connection.usb')
    expect(connectionLabel('unknown', key)).toBe('connection.device')
  })

  it('uses the configured effect tone for session states', () => {
    expect(stateTone('idle', { idle: 'calm' })).toBe('green')
    expect(stateTone('working', { working: 'busy' })).toBe('yellow')
    expect(stateTone('waiting', { waiting: 'attention' })).toBe('amber')
    expect(stateTone('error', { error: 'alert' })).toBe('red')
  })

  it('formats absolute timestamps and future countdowns', () => {
    const now = Date.UTC(2026, 0, 1, 12, 0, 0)
    expect(formatDateTime(now, 'en')).toMatch(/Jan/)
    expect(formatCountdown(now + 90_000, 'en', now)).toContain('2')
    expect(formatCountdown(now + 7_200_000, 'en', now)).toContain('2')
    expect(formatDateTime(0, 'en')).toBe('-')
    expect(formatCountdown(0, 'en', now)).toBe('-')
  })
})
