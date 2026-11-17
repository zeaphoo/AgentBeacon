import { BatteryCharging, BatteryMedium, Sun, Volume2 } from 'lucide-react'
import { useI18n } from '../i18n'
import { formatAge } from '../lib/labels'
import { Badge } from './ui/badge'
import { cn } from '../lib/utils'

/// 电量读数对应的电源状态。
///
/// "正在充电" 只认后端 `charging`（由电压是否持续上升判定）：固件的 `CHG` 标志充满后仍为 1，
/// 检测脚悬空时也会误报——实测一台 AgentCore-Light 没接 USB 却一直报 `CHG:1`。
export function batteryState(battery) {
  if (!battery) return 'unknown'
  if (battery.charging) return 'charging'
  if (battery.percent != null && battery.percent >= 100) return 'full'
  return 'discharging'
}

/// 设备电量徽章：充电中为绿色，≤20% 用警示色，其余为中性色。
export function BatteryBadge({ battery, className }) {
  const { t, language } = useI18n()
  if (!battery) return null
  const percent = battery.percent
  const state = batteryState(battery)
  const low = percent != null && percent <= 20
  const detail = []
  if (battery.millivolts != null) detail.push((battery.millivolts / 100).toFixed(2))
  if (battery.current_ma != null) detail.push(battery.current_ma)
  const title = []
  if (detail.length === 2) {
    title.push(t('device.batteryDetail', { volts: detail[0], current: detail[1] }))
  }
  // CHG 可能误报，放在提示里说明来源，别让它变成界面上的断言。
  title.push(t(battery.usb_online ? 'device.chgDetected' : 'device.chgAbsent'))
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
      {percent != null ? t('device.battery', { n: percent }) : t('device.batteryUnknown')}
      {state === 'charging' ? <span>· {t('device.charging')}</span> : null}
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
