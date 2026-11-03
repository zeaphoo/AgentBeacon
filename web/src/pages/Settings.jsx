import { useEffect, useState } from 'react'
import { Info, RefreshCw, Save, Usb } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { EFFECTS, PROFILE_LABELS } from '../lib/labels'
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

const MAPPING_ROWS = [
  ['none', '无活动'],
  ['idle', '空闲'],
  ['working', '运行中'],
  ['waiting', '等待确认'],
  ['error', '错误'],
]

const PROFILES = ['mlight-single', 'mlight-3led', 'yh50-rgy-j']

function clone(value) {
  return JSON.parse(JSON.stringify(value))
}

export default function Settings() {
  const snapshot = useBeaconStore((state) => state.snapshot)
  const saveConfig = useBeaconStore((state) => state.saveConfig)
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
          正在加载配置…
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
      setStatus('设置已保存并生效')
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
            error
              ? 'border-destructive/40 bg-destructive/10 text-destructive-foreground'
              : 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200',
          )}
        >
          {error || status}
        </div>
      )}

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">状态映射</CardTitle>
            <CardDescription>聚合状态 → 语义灯效</CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {MAPPING_ROWS.map(([key, label]) => (
              <div key={key} className="flex items-center justify-between gap-4">
                <Label htmlFor={`mapping-${key}`} className="w-24 text-muted-foreground">
                  {label}
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
                        {effect.label} · {effect.description}
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
            <CardTitle className="text-base">行为</CardTitle>
            <CardDescription>覆盖时长、亮度与退出策略</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center justify-between gap-4">
              <div>
                <Label htmlFor="override-default">手动覆盖默认时长（秒）</Label>
                <p className="mt-1 text-xs text-muted-foreground">范围 1–3600</p>
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
                <Label htmlFor="brightness">MLight 亮度</Label>
                <p className="mt-1 text-xs text-muted-foreground">0–100%，重连时下发</p>
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
                <Label>退出时关闭灯效</Label>
                <p className="mt-1 text-xs text-muted-foreground">
                  服务收到 SIGTERM 后发送 off
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
                <Label>YH50 告警蜂鸣</Label>
                <p className="mt-1 text-xs text-muted-foreground">
                  alert 灯效时同时打开蜂鸣器
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
                <Label>启用 mock 设备</Label>
                <p className="mt-1 text-xs text-muted-foreground">
                  没有硬件时用于验证完整链路
                </p>
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
          <CardTitle className="text-base">设备绑定</CardTitle>
          <CardDescription>为特定设备覆盖自动识别的型号</CardDescription>
        </CardHeader>
        <CardContent>
          {realDevices.length === 0 ? (
            <div className="flex items-center gap-2 rounded-lg border border-dashed border-border/70 px-4 py-6 text-sm text-muted-foreground">
              <Usb className="h-4 w-4" />
              未发现设备；插入状态灯后可在此绑定型号。
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
                          {PROFILE_LABELS[profile]}
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
            <Info className="h-4 w-4" />
            桌面应用
          </CardTitle>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground">
          开机自启动由桌面应用管理，请在 AgentBeacon 托盘的「开机自启动」中勾选；浏览器无法修改系统登录项。
        </CardContent>
      </Card>

      <div className="flex flex-wrap gap-2">
        <Button onClick={handleSave} disabled={saving}>
          <Save className="mr-1.5 h-4 w-4" />
          {saving ? '保存中…' : '保存设置'}
        </Button>
        <Button variant="outline" onClick={handleReload}>
          <RefreshCw className="mr-1.5 h-4 w-4" />
          重新载入
        </Button>
      </div>
    </div>
  )
}
