import { describe, expect, it } from 'vitest'
import { formatUptime, stateTone } from './labels'

const t = (key, vars) => (vars ? `${key.replace('time.', '')}:${vars.n}` : key)

describe('labels', () => {
  it('humanizes uptime with the largest useful units', () => {
    expect(formatUptime(59, t)).toBe('seconds:59')
    expect(formatUptime(60, t)).toBe('minutes:1')
    expect(formatUptime(7063, t)).toBe('hours:1 minutes:57')
    expect(formatUptime(90061, t)).toBe('days:1 hours:1')
  })

  it('uses the configured effect tone for session states', () => {
    expect(stateTone('idle', { idle: 'calm' })).toBe('green')
    expect(stateTone('working', { working: 'busy' })).toBe('yellow')
    expect(stateTone('waiting', { waiting: 'attention' })).toBe('amber')
    expect(stateTone('error', { error: 'alert' })).toBe('red')
  })
})
