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
  it('never claims charging once the pack is full', () => {
    // 固件的 CHG 标志在充满后仍然为 1，甚至可能是悬空引脚误报。
    expect(batteryState(battery({ usb_online: true, percent: 82 }))).toBe('charging')
    expect(batteryState(battery({ usb_online: true, percent: 100 }))).toBe('full')
    expect(batteryState(battery({ usb_online: false, percent: 100 }))).toBe('full')
    expect(batteryState(battery({ usb_online: false, percent: 62 }))).toBe('discharging')
    expect(batteryState(null)).toBe('unknown')
  })
})

describe('BatteryBadge', () => {
  it('only shows “充电中” while the pack is not full yet', () => {
    const { rerender } = render(
      <BatteryBadge battery={battery({ usb_online: true, percent: 82 })} />,
    )
    expect(screen.getByText(/电量 50%|电量 82%/)).toBeInTheDocument()
    expect(screen.getByText(/充电中/)).toBeInTheDocument()

    // 满电 + CHG=1：不再声称正在充电，也不声称 USB 已接入。
    rerender(<BatteryBadge battery={battery({ usb_online: true, percent: 100 })} />)
    expect(screen.queryByText(/充电中/)).not.toBeInTheDocument()
    expect(screen.queryByText(/USB 已接入/)).not.toBeInTheDocument()

    // 电池供电且电量低：给出低电量提示。
    rerender(<BatteryBadge battery={battery({ usb_online: false, percent: 14 })} />)
    expect(screen.getByText(/电量偏低/)).toBeInTheDocument()
  })

  it('keeps the raw CHG flag in the tooltip instead of asserting it', () => {
    render(<BatteryBadge battery={battery({ usb_online: true, percent: 100 })} />)
    const badge = screen.getByText(/电量 50%|电量 100%/).closest('div')
    expect(badge.getAttribute('title')).toContain('充电输入标志：已检测')
  })
})
