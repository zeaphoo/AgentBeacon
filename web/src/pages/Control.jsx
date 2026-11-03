import { useEffect, useState } from 'react'
import { Clock, Lightbulb, RotateCcw } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useI18n } from '../i18n'
import {
  EFFECTS,
  TONE_CLASSES,
  effectLabel,
} from '../lib/labels'
import { Badge } from '../components/ui/badge'
import { Button } from '../components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '../components/ui/card'
import { Input } from '../components/ui/input'
import { Label } from '../components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../components/ui/select'
import { cn } from '../lib/utils'

export default function Control() {
  const snapshot = useBeaconStore((state) => state.snapshot)
  const applyControl = useBeaconStore((state) => state.applyControl)
  const cancelControl = useBeaconStore((state) => state.cancelControl)
  const { t } = useI18n()
  const [selected, setSelected] = useState('busy')
  const [duration, setDuration] = useState('60')
  const [device, setDevice] = useState('all')
  const [message, setMessage] = useState(null)
  const [error, setError] = useState(null)
  const [now, setNow] = useState(Date.now())

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 500)
    return () => clearInterval(timer)
  }, [])

  useEffect(() => {
    if (snapshot?.config?.manual_override_default_secs && duration === '60') {
      setDuration(String(snapshot.config.manual_override_default_secs))
    }
  }, [snapshot?.config?.manual_override_default_secs])

  const devices = snapshot?.devices ?? []
  const override = snapshot?.override
  const remaining = override
    ? Math.max(0, Math.ceil((override.expires_at_ms - now) / 1000))
    : 0

  const messageText = message
    ? message.key === 'control.applied'
      ? t(message.key, {
          effect: effectLabel(message.effect, t),
          n: message.n,
        })
      : t(message.key)
    : null

  async function handleApply() {
    setError(null)
    setMessage(null)
    try {
      await applyControl({
        effect: selected,
        duration_secs: Number(duration),
        device: device === 'all' ? null : device,
      })
      setMessage({ key: 'control.applied', effect: selected, n: duration })
    } catch (err) {
      setError(err.message)
    }
  }

  async function handleCancel() {
    setError(null)
    setMessage(null)
    try {
      await cancelControl()
      setMessage({ key: 'control.restored' })
    } catch (err) {
      setError(err.message)
    }
  }

  return (
    <div className="grid gap-6 lg:grid-cols-[1.5fr_1fr]">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Lightbulb className="h-4 w-4" />
            {t('control.title')}
          </CardTitle>
          <CardDescription>{t('control.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-5">
          {(message || error) && (
            <div
              className={cn(
                'rounded-lg border px-4 py-3 text-sm',
                error ? 'alert-danger' : 'alert-success',
              )}
            >
              {error || messageText}
            </div>
          )}

          <div className="grid gap-3 sm:grid-cols-2">
            {EFFECTS.map((effect) => (
              <button
                key={effect.value}
                type="button"
                onClick={() => setSelected(effect.value)}
                className={cn(
                  'rounded-lg border p-4 text-left transition-all',
                  TONE_CLASSES[effect.tone],
                  selected === effect.value
                    ? 'ring-2 ring-primary ring-offset-2 ring-offset-background'
                    : 'opacity-80 hover:opacity-100',
                )}
              >
                <div className="flex items-center justify-between">
                  <span className="text-sm font-semibold">{t(effect.labelKey)}</span>
                  {selected === effect.value && (
                    <Badge variant="secondary">{t('control.effectSelected')}</Badge>
                  )}
                </div>
                <div className="mt-1 text-xs opacity-80">{t(effect.descriptionKey)}</div>
              </button>
            ))}
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="duration">{t('control.duration')}</Label>
              <Input
                id="duration"
                type="number"
                min={1}
                max={3600}
                value={duration}
                onChange={(event) => setDuration(event.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label>{t('control.target')}</Label>
              <Select value={device} onValueChange={setDevice}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">{t('control.allDevices')}</SelectItem>
                  {devices.map((item) => (
                    <SelectItem key={item.id} value={item.id}>
                      {item.id} · {item.profile}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="flex flex-wrap gap-2">
            <Button onClick={handleApply}>
              <Lightbulb className="mr-1.5 h-4 w-4" />
              {t('control.apply')}
            </Button>
            <Button variant="outline" onClick={handleCancel} disabled={!override}>
              <RotateCcw className="mr-1.5 h-4 w-4" />
              {t('control.restore')}
            </Button>
          </div>
        </CardContent>
      </Card>

      <div className="space-y-6">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Clock className="h-4 w-4" />
              {t('control.currentStatus')}
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">{t('control.autoEffect')}</span>
              <Badge variant="outline">
                {snapshot?.effect ? effectLabel(snapshot.effect, t) : '—'}
              </Badge>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">{t('control.manualOverride')}</span>
              <span>
                {override ? effectLabel(override.effect, t) : t('control.notEnabled')}
              </span>
            </div>
            {override && (
              <div className="alert-warning rounded-lg border p-3">
                <div className="text-xs">
                  {t('control.remaining', { n: remaining })}
                </div>
                <div className="mt-1 text-xs opacity-70">
                  {override.device
                    ? t('control.onlyDevice', { device: override.device })
                    : t('control.allTargets')}
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('control.tips')}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-xs text-muted-foreground">
            <p>{t('control.tip1')}</p>
            <p>{t('control.tip2')}</p>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
