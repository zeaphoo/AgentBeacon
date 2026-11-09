import { useEffect, useRef, useState } from 'react'
import { AlertTriangle, Lightbulb, Server, Usb } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useI18n } from '../i18n'
import {
  AGGREGATE_KEYS,
  EFFECTS,
  SUPPORTED_LIGHT_PROFILES,
  capabilitiesForProfile,
  profileLabel,
} from '../lib/labels'
import DeviceFeatures from '../components/DeviceFeatures'
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
import { Switch } from '../components/ui/switch'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'

const MAPPING_ROWS = ['none', 'idle', 'working', 'waiting', 'error']
const PROFILES = ['mlight-single', 'mlight-3led', 'yh50-rgy-j']

function clone(value) {
  return JSON.parse(JSON.stringify(value))
}

export default function Settings() {
  const snapshot = useBeaconStore((state) => state.snapshot)
  const saveConfig = useBeaconStore((state) => state.saveConfig)
  const { t } = useI18n()
  const [form, setForm] = useState(null)
  const [saving, setSaving] = useState(false)
  const [status, setStatus] = useState(null)
  const [error, setError] = useState(null)
  const [buzzerDialogOpen, setBuzzerDialogOpen] = useState(false)
  const savedRef = useRef(null)
  const requestRef = useRef(0)
  // integrations 由“Agent 集成”页面与本页共写同一份配置；保存时始终采用
  // 服务端最新值，避免本页的旧副本把自动安装开关改回去。
  const integrationsRef = useRef(null)

  useEffect(() => {
    if (snapshot?.config?.integrations) {
      integrationsRef.current = snapshot.config.integrations
    }
  }, [snapshot])

  useEffect(() => {
    if (snapshot?.config && form === null) {
      setForm(clone(snapshot.config))
    }
  }, [snapshot, form])

  useEffect(() => {
    if (!form) return

    const payload = {
      ...form,
      integrations: integrationsRef.current ?? form.integrations,
      manual_override_default_secs: Number(form.manual_override_default_secs),
      mlight: { ...form.mlight, brightness: Number(form.mlight.brightness) },
    }
    const serialized = JSON.stringify(payload)

    // The first form load is the baseline, not a change to save.
    if (savedRef.current === null) {
      savedRef.current = serialized
      return
    }
    if (serialized === savedRef.current) return

    const changeId = ++requestRef.current
    setStatus(null)
    setError(null)

    const timer = setTimeout(async () => {
      setSaving(true)
      try {
        await saveConfig(payload)
        if (changeId === requestRef.current) {
          savedRef.current = serialized
          setStatus('saved')
        }
      } catch (err) {
        if (changeId === requestRef.current) setError(err.message)
      } finally {
        if (changeId === requestRef.current) setSaving(false)
      }
    }, 500)

    return () => clearTimeout(timer)
  }, [form, saveConfig])

  if (!form) {
    return (
      <Card>
        <CardContent className="py-10 text-center text-sm text-muted-foreground">
          {t('settings.loading')}
        </CardContent>
      </Card>
    )
  }

  const realDevices = (snapshot?.devices ?? []).filter((device) => device.profile !== 'mock')
  const supportedProfiles = (
    snapshot?.profiles?.length
      ? snapshot.profiles
      : SUPPORTED_LIGHT_PROFILES.map((profile) => ({
          profile,
          capabilities: capabilitiesForProfile(profile),
        }))
  ).filter((item) => item.profile !== 'mock')

  function update(patch) {
    setForm((current) => ({ ...current, ...patch }))
  }

  function bindingFor(device) {
    const serial = device.id.startsWith('serial:') ? device.id.slice('serial:'.length) : null
    return form.devices.find((binding) =>
      serial ? binding.serial === serial : binding.port === device.port,
    )
  }

  function profileFor(device) {
    return bindingFor(device)?.profile ?? device.profile
  }

  function setDeviceProfile(device, profile) {
    const serial = device.id.startsWith('serial:') ? device.id.slice('serial:'.length) : null
    setForm((current) => {
      const bindings = current.devices.filter((binding) =>
        serial ? binding.serial !== serial : binding.port !== device.port,
      )
      const entry = serial
        ? { serial, port: null, profile }
        : { port: device.port, serial: null, profile }
      return { ...current, devices: [...bindings, entry] }
    })
  }

  return (
    <div className="space-y-6">
      {error && (
        <div className="alert-danger rounded-lg border px-4 py-3 text-sm" role="alert">
          {error}
        </div>
      )}

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('settings.mappingTitle')}</CardTitle>
            <CardDescription>{t('settings.mappingDescription')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {MAPPING_ROWS.map((key) => (
              <div key={key} className="flex items-center justify-between gap-4">
                <Label htmlFor={`mapping-${key}`} className="w-32 text-muted-foreground">
                  {t(AGGREGATE_KEYS[key])}
                </Label>
                <Select
                  value={form.mapping[key]}
                  onValueChange={(value) =>
                    update({ mapping: { ...form.mapping, [key]: value } })
                  }
                >
                  <SelectTrigger id={`mapping-${key}`} className="max-w-52">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {EFFECTS.map((effect) => (
                      <SelectItem key={effect.value} value={effect.value}>
                        {t(effect.labelKey)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            ))}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('settings.behaviorTitle')}</CardTitle>
            <CardDescription>{t('settings.behaviorDescription')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center justify-between gap-4">
              <div>
                <Label htmlFor="override-default">{t('settings.overrideDefault')}</Label>
                <p className="mt-1 text-xs text-muted-foreground">{t('settings.range')}</p>
              </div>
              <Input
                id="override-default"
                type="number"
                min={1}
                max={3600}
                className="w-24"
                value={form.manual_override_default_secs}
                onChange={(event) =>
                  update({ manual_override_default_secs: event.target.value })
                }
              />
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <Label htmlFor="brightness">{t('settings.brightness')}</Label>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t('settings.brightnessHint')}
                </p>
              </div>
              <Input
                id="brightness"
                type="number"
                min={0}
                max={100}
                className="w-24"
                value={form.mlight.brightness}
                onChange={(event) =>
                  update({ mlight: { ...form.mlight, brightness: event.target.value } })
                }
              />
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <Label>{t('settings.shutdownOff')}</Label>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t('settings.shutdownOffHint')}
                </p>
              </div>
              <Switch
                checked={form.shutdown.turn_off_lights}
                onCheckedChange={(checked) =>
                  update({ shutdown: { ...form.shutdown, turn_off_lights: checked } })
                }
              />
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <Label htmlFor="buzzer">{t('settings.buzzer')}</Label>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t('settings.buzzerHint')}
                </p>
              </div>
              <Switch
                id="buzzer"
                checked={form.yh50.buzzer_on_alert}
                onCheckedChange={(checked) => {
                  if (checked && !form.yh50.buzzer_on_alert) {
                    setBuzzerDialogOpen(true)
                  } else {
                    update({ yh50: { ...form.yh50, buzzer_on_alert: checked } })
                  }
                }}
              />
            </div>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.devicesTitle')}</CardTitle>
          <CardDescription>{t('settings.devicesDescription')}</CardDescription>
        </CardHeader>
        <CardContent>
          {realDevices.length === 0 ? (
            <div className="flex items-center gap-2 rounded-lg border border-dashed border-border/70 px-4 py-6 text-sm text-muted-foreground">
              <Usb className="h-4 w-4" />
              {t('settings.noDevices')}
            </div>
          ) : (
            <div className="space-y-3">
              {realDevices.map((device) => (
                <div
                  key={device.id}
                  className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border/70 bg-background/40 px-4 py-3"
                >
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium">{device.id}</div>
                    <div className="truncate text-xs text-muted-foreground">{device.port}</div>
                    <DeviceFeatures
                      capabilities={capabilitiesForProfile(
                        profileFor(device),
                        snapshot?.profiles,
                      )}
                      className="mt-1.5"
                    />
                  </div>
                  <Select
                    value={profileFor(device)}
                    onValueChange={(value) => setDeviceProfile(device, value)}
                  >
                    <SelectTrigger className="w-52">
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
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Lightbulb className="h-4 w-4" />
            {t('settings.supportedTitle')}
          </CardTitle>
          <CardDescription>{t('settings.supportedDescription')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
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
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Server className="h-4 w-4" />
            {t('settings.serviceTitle')}
          </CardTitle>
          <CardDescription>{t('settings.serviceDescription')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-1 text-xs text-muted-foreground">
          <div>127.0.0.1:{snapshot?.service?.port ?? 6980}</div>
          <div>
            {snapshot?.service?.version ? `v${snapshot.service.version}` : 'agent-beacon'}
          </div>
        </CardContent>
      </Card>

      <Dialog open={buzzerDialogOpen} onOpenChange={setBuzzerDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-4 w-4 text-warning" />
              {t('settings.buzzerConfirmTitle')}
            </DialogTitle>
            <DialogDescription>{t('settings.buzzerConfirmDescription')}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="ghost">{t('common.cancel')}</Button>
            </DialogClose>
            <DialogClose asChild>
              <Button
                onClick={() =>
                  update({ yh50: { ...form.yh50, buzzer_on_alert: true } })
                }
              >
                {t('settings.buzzerConfirm')}
              </Button>
            </DialogClose>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <p className="text-xs text-muted-foreground" aria-live="polite">
        {saving
          ? t('settings.saving')
          : status
            ? t('settings.saved')
            : t('settings.autoSaveHint')}
      </p>
    </div>
  )
}
