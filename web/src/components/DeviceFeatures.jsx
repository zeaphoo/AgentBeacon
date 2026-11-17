import { BatteryCharging, BatteryMedium, Sun, Volume2 } from 'lucide-react'
import { useI18n } from '../i18n'
import { Badge } from './ui/badge'
import { cn } from '../lib/utils'

/// 设备电量徽章：充电中为绿色，低于 20% 用警示色，其余为中性色。
export function BatteryBadge({ battery, className }) {
  const { t } = useI18n()
  if (!battery) return null
  const percent = battery.percent
  const low = !battery.charging && percent != null && percent <= 20
  const detail = []
  if (battery.millivolts != null) detail.push((battery.millivolts / 100).toFixed(2))
  if (battery.current_ma != null) detail.push(battery.current_ma)
  const title =
    detail.length === 2
      ? t('device.batteryDetail', { volts: detail[0], current: detail[1] })
      : undefined
  const Icon = battery.charging ? BatteryCharging : BatteryMedium
  return (
    <Badge
      variant="outline"
      title={title}
      className={cn(
        'gap-1 px-2 py-0 text-[11px] font-normal',
        battery.charging
          ? 'tone-green border'
          : low
            ? 'tone-amber border'
            : 'text-muted-foreground',
        className,
      )}
    >
      <Icon className="h-3 w-3" />
      {percent != null ? t('device.battery', { n: percent }) : t('device.batteryUnknown')}
      {battery.charging ? <span>· {t('device.charging')}</span> : null}
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
    <div
      className={cn(
        'flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground',
        className,
      )}
    >
      {hasName && (
        <span className="truncate font-mono" title={t('device.hardwareName', { name: device.name })}>
          {device.name}
        </span>
      )}
      {hasFirmware && (
        <span className="truncate">{t('device.firmware', { version: device.firmware })}</span>
      )}
      <BatteryBadge battery={device.battery} />
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
