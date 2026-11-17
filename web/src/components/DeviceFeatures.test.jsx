import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { BatteryBadge, batteryState } from './DeviceFeatures'

const battery = (overrides) => ({
  percent: 50,
  usb_online: false,
  charging: false,
  millivolts: 380,
  current_ma: null,
  updated_at_ms: Date.now(),
  ...overrides,
})

describe('batteryState', () => {
  it('only trusts the derived charging flag, never the raw CHG bit', () => {
    // CHG=1 但后端判定没在充电（电压稳定）→ 不能说“充电中”。
    expect(batteryState(battery({ usb_online: true, charging: false, percent: 96 }))).toBe(
      'discharging',
    )
    expect(batteryState(battery({ usb_online: true, charging: true, percent: 62 }))).toBe(
      'charging',
    )
    expect(batteryState(battery({ charging: false, percent: 100 }))).toBe('full')
    expect(batteryState(null)).toBe('unknown')
  })
})

describe('BatteryBadge', () => {
  it('shows “充电中” only when the backend detected a real charge', () => {
    const { rerender } = render(
      <BatteryBadge battery={battery({ usb_online: true, charging: true, percent: 62 })} />,
    )
    expect(screen.getByText(/· 充电中/)).toBeInTheDocument()

    // CHG=1 但电压没上升：不显示充电，也不声称 USB 已接入。
    rerender(
      <BatteryBadge battery={battery({ usb_online: true, charging: false, percent: 96 })} />,
    )
    expect(screen.queryByText(/充电中/)).not.toBeInTheDocument()
    expect(screen.queryByText(/USB 已接入/)).not.toBeInTheDocument()

    // 电池供电且电量低：给出低电量提示。
    rerender(<BatteryBadge battery={battery({ percent: 14 })} />)
    expect(screen.getByText(/电量偏低/)).toBeInTheDocument()
  })

  it('keeps the raw CHG flag in the tooltip instead of asserting it', () => {
    render(<BatteryBadge battery={battery({ usb_online: true, percent: 96 })} />)
    const badge = screen.getByText(/电量 96%/).closest('div')
    expect(badge.getAttribute('title')).toContain('充电输入标志：已检测')
  })
})
