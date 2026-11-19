import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { BatteryBadge, batteryState } from './DeviceFeatures'

const battery = (overrides) => ({
  percent: 50,
  chg_flag: false,
  charging: false,
  millivolts: 380,
  current_ma: null,
  updated_at_ms: Date.now(),
  ...overrides,
})

describe('batteryState', () => {
  it('only shows charging when the backend derived it from voltage evidence', () => {
    // `CHG=1` 但静置电压稳定 → 后端 charging=false → 不显示充电。
    expect(batteryState(battery({ chg_flag: true, charging: false, percent: 96 }))).toBe(
      'discharging',
    )
    expect(batteryState(battery({ charging: true, percent: 62 }))).toBe('charging')
    // 已充满要「充电中 + 100%」，不到 100% 不算。
    expect(batteryState(battery({ charging: true, percent: 99 }))).toBe('charging')
    expect(batteryState(battery({ charging: true, percent: 100 }))).toBe('full')
    expect(batteryState(battery({ charging: false, percent: 100 }))).toBe('discharging')
    expect(batteryState(null)).toBe('unknown')
  })
})

describe('BatteryBadge', () => {
  it('shows “充电中” only when the backend confirmed USB power', () => {
    const { rerender } = render(
      <BatteryBadge battery={battery({ chg_flag: true, charging: true, percent: 62 })} />,
    )
    expect(screen.getByText(/· 充电中/)).toBeInTheDocument()

    // 只有 CHG、证据不足（后端 usb_online=false）：不显示充电。
    rerender(
      <BatteryBadge battery={battery({ chg_flag: true, charging: false, percent: 96 })} />,
    )
    expect(screen.queryByText(/充电中/)).not.toBeInTheDocument()

    // 电池供电且电量低：给出低电量提示。
    rerender(<BatteryBadge battery={battery({ percent: 14 })} />)
    expect(screen.getByText(/电量偏低/)).toBeInTheDocument()
  })

  it('keeps the raw CHG flag in the tooltip instead of asserting it', () => {
    render(<BatteryBadge battery={battery({ chg_flag: true, percent: 96 })} />)
    const badge = screen.getByText(/电量 96%/).closest('div')
    expect(badge.getAttribute('title')).toContain('充电输入标志：已检测')
  })

  it('says 已充满 only at 100% while USB is online', () => {
    const { rerender } = render(
      <BatteryBadge battery={battery({ charging: true, percent: 100 })} />,
    )
    expect(screen.getByText(/已充满/)).toBeInTheDocument()

    // 99% 不能显示“已充满”。
    rerender(<BatteryBadge battery={battery({ charging: true, percent: 99 })} />)
    expect(screen.queryByText(/已充满/)).not.toBeInTheDocument()
  })

  it('marks model-driven percentages with ≈', () => {
    render(
      <BatteryBadge
        battery={battery({ charging: true, percent: 72, percent_estimated: true })}
      />,
    )
    expect(screen.getByText(/≈72%/)).toBeInTheDocument()
  })

  it('shows 电量未知 when the device reports nothing usable', () => {
    render(<BatteryBadge battery={battery({ percent: null, millivolts: 266 })} />)
    expect(screen.getByText(/电量未知/)).toBeInTheDocument()
  })

  it('surfaces the raw gauge reading when it disagrees with the shown percent', () => {
    render(
      <BatteryBadge
        battery={battery({ usb_online: true, percent: 100, gauge_percent: 0, millivolts: 402 })}
      />,
    )
    const badge = screen.getByText(/电量 100%/).closest('div')
    expect(badge.getAttribute('title')).toContain('电量计读数 0%')
  })
})
