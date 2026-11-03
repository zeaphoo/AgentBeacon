import { useEffect, useState } from 'react'
import { Clock, Lightbulb, RotateCcw } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { EFFECTS, EFFECT_MAP, TONE_CLASSES } from '../lib/labels'
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

  async function handleApply() {
    setError(null)
    setMessage(null)
    try {
      await applyControl({
        effect: selected,
        duration_secs: Number(duration),
        device: device === 'all' ? null : device,
      })
      setMessage(`已应用 ${EFFECT_MAP[selected]?.label}，${duration} 秒后恢复自动`)
    } catch (err) {
      setError(err.message)
    }
  }

  async function handleCancel() {
    setError(null)
    setMessage(null)
    try {
      await cancelControl()
      setMessage('已恢复自动灯效')
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
            手动灯光控制
          </CardTitle>
          <CardDescription>覆盖自动映射，到期后自动恢复</CardDescription>
        </CardHeader>
        <CardContent className="space-y-5">
          {(message || error) && (
            <div
              className={cn(
                'rounded-lg border px-4 py-3 text-sm',
                error
                  ? 'border-destructive/40 bg-destructive/10 text-destructive-foreground'
                  : 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200',
              )}
            >
              {error || message}
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
                  <span className="text-sm font-semibold">{effect.label}</span>
                  {selected === effect.value && <Badge variant="secondary">已选</Badge>}
                </div>
                <div className="mt-1 text-xs opacity-80">{effect.description}</div>
              </button>
            ))}
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="duration">持续时长（秒）</Label>
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
              <Label>作用设备</Label>
              <Select value={device} onValueChange={setDevice}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">全部设备</SelectItem>
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
              应用灯效
            </Button>
            <Button variant="outline" onClick={handleCancel} disabled={!override}>
              <RotateCcw className="mr-1.5 h-4 w-4" />
              恢复自动
            </Button>
          </div>
        </CardContent>
      </Card>

      <div className="space-y-6">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Clock className="h-4 w-4" />
              当前状态
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">自动灯效</span>
              <Badge variant="outline">
                {EFFECT_MAP[snapshot?.effect]?.label ?? '—'}
              </Badge>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">手动覆盖</span>
              <span>{override ? `${EFFECT_MAP[override.effect]?.label}` : '未启用'}</span>
            </div>
            {override && (
              <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-amber-100">
                <div className="text-xs">剩余 {remaining} 秒后恢复自动</div>
                <div className="mt-1 text-xs opacity-70">
                  {override.device ? `仅作用于 ${override.device}` : '作用于全部设备'}
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">提示</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-xs text-muted-foreground">
            <p>手动覆盖不会修改或清空 agent 会话，覆盖期间聚合状态仍会继续更新。</p>
            <p>同一设备重复应用相同灯效会被抑制，避免打断进行中的动画。</p>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
