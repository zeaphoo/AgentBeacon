import { Sun, Volume2 } from 'lucide-react'
import { useI18n } from '../i18n'
import { Badge } from './ui/badge'
import { cn } from '../lib/utils'

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
