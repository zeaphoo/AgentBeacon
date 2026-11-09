import { useEffect, useRef, useState } from 'react'
import { BookOpen, Clock, Lightbulb, RefreshCw, RotateCcw, Usb } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useI18n } from '../i18n'
import {
  EFFECTS,
  SUPPORTED_LIGHT_PROFILES,
  TONE_CLASSES,
  capabilitiesForProfile,
  effectLabel,
  profileLabel,
} from '../lib/labels'
import DeviceFeatures from '../components/DeviceFeatures'
import { Badge } from '../components/ui/badge'
import { Button } from '../components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '../components/ui/card'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'
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

const PROFILES = ['mlight-single', 'mlight-3led', 'yh50-rgy-j', 'agentcore-light']

export default function Control() {
  const snapshot = useBeaconStore((state) => state.snapshot)
  const applyControl = useBeaconStore((state) => state.applyControl)
  const cancelControl = useBeaconStore((state) => state.cancelControl)
  const scanDevices = useBeaconStore((state) => state.scanDevices)
  const saveConfig = useBeaconStore((state) => state.saveConfig)
  const { t } = useI18n()
  const [selected, setSelected] = useState('busy')
  const [duration, setDuration] = useState('60')
  const [device, setDevice] = useState('all')
  const [message, setMessage] = useState(null)
  const [error, setError] = useState(null)
  const [now, setNow] = useState(Date.now())
  const [scanning, setScanning] = useState(false)
  const [devices, setDevices] = useState(null)
  const [bindingStatus, setBindingStatus] = useState(null)
  const [supportedOpen, setSupportedOpen] = useState(false)
  const savedBindingsRef = useRef(null)

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 500)
    return () => clearInterval(timer)
  }, [])

  useEffect(() => {
    if (snapshot?.config?.manual_override_default_secs && duration === '60') {
      setDuration(String(snapshot.config.manual_override_default_secs))
    }
  }, [snapshot?.config?.manual_override_default_secs])

  useEffect(() => {
    if (snapshot?.config && devices === null) {
      setDevices(snapshot.config.devices ?? [])
    }
  }, [snapshot, devices])

  // 设备 profile 绑定与设置页共用同一份配置：保存时以服务端最新配置为基准，
  // 只覆盖 devices 字段，避免与其他页面的修改互相覆盖。
  useEffect(() => {
    if (!devices || !snapshot?.config) return

    const serialized = JSON.stringify(devices)

    if (savedBindingsRef.current === null) {
      savedBindingsRef.current = serialized
      return
    }
    if (serialized === savedBindingsRef.current) return

    const payload = {
      ...snapshot.config,
      devices,
      manual_override_default_secs: Number(snapshot.config.manual_override_default_secs),
      mlight: {
        ...snapshot.config.mlight,
        brightness: Number(snapshot.config.mlight?.brightness),
      },
    }

    setBindingStatus('saving')
    saveConfig(payload)
      .then(() => (savedBindingsRef.current = serialized))
      .then(() => setBindingStatus('saved'))
      .catch((err) => setBindingStatus(err.message))
  }, [devices, snapshot?.config, saveConfig])

  const deviceList = snapshot?.devices ?? []
  // mock 设备只在状态页与手动覆盖目标中出现，不参与型号绑定。
  const realDevices = deviceList.filter((item) => item.profile !== 'mock')
  const override = snapshot?.override
  const remaining = override
    ? Math.max(0, Math.ceil((override.expires_at_ms - now) / 1000))
    : 0

  const supportedProfiles = (
    snapshot?.profiles?.length
      ? snapshot.profiles
      : SUPPORTED_LIGHT_PROFILES.map((profile) => ({
          profile,
          capabilities: capabilitiesForProfile(profile),
        }))
  ).filter((item) => item.profile !== 'mock')

  const messageText = message
    ? message.key === 'control.applied'
      ? t(message.key, { effect: effectLabel(message.effect, t), n: message.n })
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

  async function handleScan() {
    setScanning(true)
    setError(null)
    try {
      await scanDevices()
    } catch (err) {
      setError(err.message)
    } finally {
      setScanning(false)
    }
  }

  function bindingFor(item) {
    const serial = item.id.startsWith('serial:') ? item.id.slice('serial:'.length) : null
    return (devices ?? []).find((binding) =>
      serial ? binding.serial === serial : binding.port === item.port,
    )
  }

  function profileFor(item) {
    return bindingFor(item)?.profile ?? item.profile
  }

  function setDeviceProfile(item, profile) {
    const serial = item.id.startsWith('serial:') ? item.id.slice('serial:'.length) : null
    setDevices((current) => {
      const bindings = (current ?? []).filter((binding) =>
        serial ? binding.serial !== serial : binding.port !== item.port,
      )
      const entry = serial
        ? { serial, port: null, profile }
        : { port: item.port, serial: null, profile }
      return [...bindings, entry]
    })
  }

  return (
    <div className="space-y-6">
      {error && (
        <div className="alert-danger rounded-lg border px-4 py-3 text-sm" role="alert">
          {error}
        </div>
      )}

      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Lightbulb className="h-4 w-4" />
              {t('control.title')}
            </CardTitle>
            <CardDescription>{t('control.description')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {messageText && (
              <div className="alert-success rounded-lg border px-4 py-2.5 text-sm">
                {messageText}
              </div>
            )}

            <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
              {EFFECTS.map((effect) => (
                <button
                  key={effect.value}
                  type="button"
                  onClick={() => setSelected(effect.value)}
                  aria-pressed={selected === effect.value}
                  className={cn(
                    'rounded-lg border px-3 py-2.5 text-left transition-all',
                    TONE_CLASSES[effect.tone],
                    selected === effect.value
                      ? 'ring-2 ring-primary ring-offset-2 ring-offset-background'
                      : 'opacity-75 hover:opacity-100',
                  )}
                >
                  <span className="block text-sm font-semibold">{t(effect.labelKey)}</span>
                  <span className="mt-0.5 block text-[11px] leading-snug opacity-80">
                    {t(effect.descriptionKey)}
                  </span>
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
                <Label htmlFor="target">{t('control.target')}</Label>
                <Select value={device} onValueChange={setDevice}>
                  <SelectTrigger id="target">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">{t('control.allDevices')}</SelectItem>
                    {deviceList.map((item) => (
                      <SelectItem key={item.id} value={item.id}>
                        {item.id} · {profileLabel(profileFor(item), t)}
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

            <div className="space-y-1 border-t border-border/70 pt-3 text-xs text-muted-foreground">
              <p>{t('control.tip1')}</p>
              <p>{t('control.tip2')}</p>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Clock className="h-4 w-4" />
              {t('control.currentStatus')}
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <div className="flex items-center justify-between gap-3">
              <span className="text-muted-foreground">{t('control.autoEffect')}</span>
              <Badge variant="outline">
                {snapshot?.effect ? effectLabel(snapshot.effect, t) : '—'}
              </Badge>
            </div>
            <div className="flex items-center justify-between gap-3">
              <span className="text-muted-foreground">{t('control.manualOverride')}</span>
              <span>
                {override ? effectLabel(override.effect, t) : t('control.notEnabled')}
              </span>
            </div>
            {override && (
              <div className="alert-warning rounded-lg border p-3">
                <div className="text-xs">{t('control.remaining', { n: remaining })}</div>
                <div className="mt-1 text-xs opacity-70">
                  {override.device
                    ? t('control.onlyDevice', { device: override.device })
                    : t('control.allTargets')}
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader className="flex-row flex-wrap items-start justify-between gap-3 space-y-0">
          <div>
            <CardTitle className="text-base">{t('lights.devicesTitle')}</CardTitle>
            <CardDescription>{t('lights.devicesDescription')}</CardDescription>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={() => setSupportedOpen(true)}>
              <BookOpen className="mr-1.5 h-3.5 w-3.5" />
              {t('lights.supportedButton')}
            </Button>
            <Button variant="outline" size="sm" onClick={handleScan} disabled={scanning}>
              <RefreshCw className={cn('mr-1.5 h-3.5 w-3.5', scanning && 'animate-spin')} />
              {scanning ? t('dashboard.scanning') : t('dashboard.scan')}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          {realDevices.length === 0 ? (
            <div className="flex items-center gap-2 rounded-lg border border-dashed border-border/70 px-4 py-6 text-sm text-muted-foreground">
              <Usb className="h-4 w-4" />
              {t('lights.noDevices')}
            </div>
          ) : (
            <ul className="divide-y divide-border/70">
              {realDevices.map((item) => (
                <li
                  key={item.id}
                  className="flex flex-wrap items-center justify-between gap-3 py-3"
                >
                  <div className="flex min-w-0 items-start gap-3">
                    <Usb className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="truncate text-sm font-medium">
                          {profileLabel(item.profile, t)}
                        </span>
                        <Badge variant={item.connected ? 'default' : 'secondary'}>
                          {item.connected
                            ? t('dashboard.connected')
                            : t('dashboard.disconnected')}
                        </Badge>
                      </div>
                      <div className="mt-0.5 truncate font-mono text-xs text-muted-foreground">
                        {item.port}
                      </div>
                      <DeviceFeatures
                        capabilities={capabilitiesForProfile(
                          profileFor(item),
                          snapshot?.profiles,
                        )}
                        className="mt-1.5"
                      />
                      {item.last_error && (
                        <div className="mt-1 text-xs text-danger">
                          {t('dashboard.deviceError', { error: item.last_error })}
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="text-xs text-muted-foreground">
                      {t('lights.deviceEffect', {
                        effect: effectLabel(item.effect, t),
                      })}
                    </span>
                    <Select
                      value={profileFor(item)}
                      onValueChange={(value) => setDeviceProfile(item, value)}
                    >
                      <SelectTrigger
                        className="w-44"
                        aria-label={t('lights.profileFor', { device: item.port })}
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {PROFILES.map((profile) => (
                          <SelectItem key={profile} value={profile}>
                            {profileLabel(profile, t)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                </li>
              ))}
            </ul>
          )}

          {bindingStatus && (
            <p className="text-xs text-muted-foreground" aria-live="polite">
              {bindingStatus === 'saving'
                ? t('settings.saving')
                : bindingStatus === 'saved'
                  ? t('settings.saved')
                  : bindingStatus}
            </p>
          )}
        </CardContent>
      </Card>

      <Dialog open={supportedOpen} onOpenChange={setSupportedOpen}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Lightbulb className="h-4 w-4" />
              {t('lights.supportedTitle')}
            </DialogTitle>
            <DialogDescription>{t('lights.supportedDescription')}</DialogDescription>
          </DialogHeader>
          <div className="max-h-[60vh] space-y-3 overflow-y-auto pr-1">
            {supportedProfiles.map((item) => (
              <div
                key={item.profile}
                className="rounded-lg border border-border/70 bg-background/40 p-4"
              >
                <div className="text-sm font-medium">{profileLabel(item.profile, t)}</div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t(`profile.${item.profile}.description`)}
                </p>
                <DeviceFeatures capabilities={item.capabilities} className="mt-2" />
              </div>
            ))}
          </div>
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="outline">{t('common.close')}</Button>
            </DialogClose>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
