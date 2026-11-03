import { useEffect, useState } from 'react'
import { RefreshCw, Save, Usb } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useI18n } from '../i18n'
import { AGGREGATE_KEYS, EFFECTS, profileLabel } from '../lib/labels'
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
import { cn } from '../lib/utils'

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

  useEffect(() => {
    if (snapshot?.config && form === null) {
      setForm(clone(snapshot.config))
    }
  }, [snapshot, form])

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

  async function handleSave() {
    setSaving(true)
    setError(null)
    setStatus(null)
    try {
      const payload = {
        ...form,
        manual_override_default_secs: Number(form.manual_override_default_secs),
        mlight: { ...form.mlight, brightness: Number(form.mlight.brightness) },
      }
      await saveConfig(payload)
      setForm(clone(payload))
      setStatus('saved')
    } catch (err) {
      setError(err.message)
    } finally {
      setSaving(false)
    }
  }

  function handleReload() {
    if (snapshot?.config) {
      setForm(clone(snapshot.config))
      setStatus(null)
      setError(null)
    }
  }

  return (
    <div className="space-y-6">
      {(status || error) && (
        <div
          className={cn(
            'rounded-lg border px-4 py-3 text-sm',
            error ? 'alert-danger' : 'alert-success',
          )}
        >
          {error || (status ? t('settings.saved') : null)}
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
                <Label>{t('settings.buzzer')}</Label>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t('settings.buzzerHint')}
                </p>
              </div>
              <Switch
                checked={form.yh50.buzzer_on_alert}
                onCheckedChange={(checked) =>
                  update({ yh50: { ...form.yh50, buzzer_on_alert: checked } })
                }
              />
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <Label>{t('settings.mock')}</Label>
                <p className="mt-1 text-xs text-muted-foreground">{t('settings.mockHint')}</p>
              </div>
              <Switch
                checked={form.mock.enabled}
                onCheckedChange={(checked) => update({ mock: { ...form.mock, enabled: checked } })}
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

      <div className="flex flex-wrap gap-2">
        <Button onClick={handleSave} disabled={saving}>
          <Save className="mr-1.5 h-4 w-4" />
          {saving ? t('settings.saving') : t('settings.save')}
        </Button>
        <Button variant="outline" onClick={handleReload}>
          <RefreshCw className="mr-1.5 h-4 w-4" />
          {t('settings.reload')}
        </Button>
      </div>
    </div>
  )
}
