import { BatteryCharging, BatteryMedium, Sun, Volume2 } from 'lucide-react'
import { useI18n } from '../i18n'
import { formatAge } from '../lib/labels'
import { Badge } from './ui/badge'
import { cn } from '../lib/utils'

/// 电量读数对应的电源状态。
///
/// “是否充电”只认后端由电压证据判定的 `charging`（`CHG` 只是诊断信息：Light 拔电也是 1）；
/// 充电中且电量到 100% 才算“已充满”。
export function batteryState(battery) {
  if (!battery) return 'unknown'
  const percent = battery.percent
  if (battery.charging && percent != null && percent >= 100) return 'full'
  if (battery.charging) return 'charging'
  return 'discharging'
}

/// 设备电量徽章：充电中为绿色，≤20% 用警示色，其余为中性色。
export function BatteryBadge({ battery, className }) {
  const { t, language } = useI18n()
  if (!battery) return null
  const percent = battery.percent
  const state = batteryState(battery)
  // 充电期间电量可能来自“容量 + 充电电流”的推算，与厂商一样标成 ≈。
  const shown = percent != null && battery.percent_estimated ? `≈${percent}` : percent
  const low = percent != null && percent <= 20
  const detail = []
  if (battery.millivolts != null) detail.push((battery.millivolts / 100).toFixed(2))
  if (battery.current_ma != null) detail.push(battery.current_ma)
  const title = []
  if (detail.length === 2) {
    title.push(t('device.batteryDetail', { volts: detail[0], current: detail[1] }))
  }
  // 电量计未校准时读数会差很远（充满报 0），把原始读数放进提示里方便核对。
  if (battery.gauge_percent != null && battery.gauge_percent !== percent) {
    title.push(t('device.gaugePercent', { n: battery.gauge_percent }))
  }
  // CHG 可能误报，放在提示里说明来源，别让它变成界面上的断言。
  title.push(t(battery.chg_flag ? 'device.chgDetected' : 'device.chgAbsent'))
  if (battery.updated_at_ms) {
    title.push(t('device.batteryUpdated', { time: formatAge(battery.updated_at_ms, language) }))
  }
  const Icon = state === 'charging' ? BatteryCharging : BatteryMedium
  return (
    <Badge
      variant="outline"
      title={title.join(' · ')}
      className={cn(
        'gap-1 px-2 py-0 text-[11px] font-normal',
        state === 'charging'
          ? 'tone-green border'
          : low
            ? 'tone-amber border'
            : 'text-muted-foreground',
        className,
      )}
    >
      <Icon className="h-3 w-3" />
      {shown != null ? t('device.battery', { n: shown }) : t('device.batteryUnknown')}
      {state === 'charging' ? <span>· {t('device.charging')}</span> : null}
      {state === 'full' ? <span>· {t('device.full')}</span> : null}
      {low ? <span>· {t('device.lowBattery')}</span> : null}
    </Badge>
  )
}

/// 硬件信息：设备自报名字、固件版本与电量。
export function DeviceTelemetry({ device, className }) {
  const { t } = useI18n()
  if (!device) return null
  const hasName = Boolean(device.name)
  const hasFirmware = Boolean(device.firmware)
  const hasBattery = Boolean(device.battery)
  if (!hasName && !hasFirmware && !hasBattery) return null
  return (
    <div className={cn('flex flex-col gap-1 text-[11px] text-muted-foreground', className)}>
      {(hasName || hasFirmware) && (
        <div className="flex flex-wrap items-center gap-1.5">
          {hasName && (
            <span
              className="truncate font-mono"
              title={t('device.hardwareName', { name: device.name })}
            >
              {device.name}
            </span>
          )}
          {hasFirmware && (
            <span className="truncate">{t('device.firmware', { version: device.firmware })}</span>
          )}
        </div>
      )}
      {hasBattery && <BatteryBadge battery={device.battery} className="self-start" />}
    </div>
  )
}

export default function DeviceFeatures({ capabilities, className }) {
  const { t } = useI18n()
  if (!capabilities) return null

  const features = []
  if (capabilities.brightness) {
    features.push({ key: 'brightness', icon: Sun, label: t('feature.brightness') })
  }
  if (capabilities.buzzer) {
    features.push({ key: 'buzzer', icon: Volume2, label: t('feature.buzzer') })
  }
  if (features.length === 0) return null

  return (
    <div className={cn('flex flex-wrap gap-1.5', className)}>
      {features.map(({ key, icon: Icon, label }) => (
        <Badge
          key={key}
          variant="outline"
          className="gap-1 px-2 py-0 text-[11px] font-normal text-muted-foreground"
        >
          <Icon className="h-3 w-3" />
          {label}
        </Badge>
      ))}
    </div>
  )
}
