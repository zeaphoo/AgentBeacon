import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { BatteryBadge, batteryState } from './DeviceFeatures'

const battery = (overrides) => ({
  percent: 50,
  usb_online: false,
  millivolts: 380,
  current_ma: null,
  updated_at_ms: Date.now(),
  ...overrides,
})

describe('batteryState', () => {
  it('treats CHG:1 as charge input, not as “charging” once the pack is full', () => {
    // 固件的 CHG 标志在充满后仍然为 1，甚至可能是悬空引脚误报。
    expect(batteryState(battery({ usb_online: true, percent: 82 }))).toBe('charging')
    expect(batteryState(battery({ usb_online: true, percent: 100 }))).toBe('full')
    expect(batteryState(battery({ usb_online: false, percent: 100 }))).toBe('discharging')
    expect(batteryState(null)).toBe('unknown')
  })
})

describe('BatteryBadge', () => {
  it('labels the three power states', () => {
    const { rerender } = render(
      <BatteryBadge battery={battery({ usb_online: true, percent: 82 })} />,
    )
    expect(screen.getByText(/USB 已接入 · 充电中/)).toBeInTheDocument()

    rerender(<BatteryBadge battery={battery({ usb_online: true, percent: 100 })} />)
    expect(screen.getByText(/USB 已接入 · 已充满/)).toBeInTheDocument()

    rerender(<BatteryBadge battery={battery({ usb_online: false, percent: 14 })} />)
    expect(screen.getByText(/电池供电/)).toBeInTheDocument()
    expect(screen.getByText(/电量偏低/)).toBeInTheDocument()
  })

  it('does not flag low battery while the light is on USB power', () => {
    render(<BatteryBadge battery={battery({ usb_online: true, percent: 12 })} />)
    expect(screen.queryByText(/电量偏低/)).not.toBeInTheDocument()
  })
})
