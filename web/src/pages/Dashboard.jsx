import { useState } from 'react'
import { Activity, Clock, Lightbulb, RefreshCw, Server, Trash2, Usb } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import {
  AGGREGATE_LABELS,
  EFFECT_MAP,
  PROFILE_LABELS,
  STATE_LABELS,
  STATE_TONE,
  TONE_CLASSES,
  effectTone,
  formatAge,
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
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '../components/ui/dialog'
import { cn } from '../lib/utils'

function EmptyState({ icon: Icon, title, description }) {
  return (
    <div className="flex flex-col items-center gap-2 rounded-lg border border-dashed border-border/70 px-6 py-10 text-center">
      <Icon className="h-6 w-6 text-muted-foreground" />
      <div className="text-sm font-medium">{title}</div>
      <div className="max-w-md text-xs text-muted-foreground">{description}</div>
    </div>
  )
}

export default function Dashboard() {
  const snapshot = useBeaconStore((state) => state.snapshot)
  const loading = useBeaconStore((state) => state.loading)
  const error = useBeaconStore((state) => state.error)
  const load = useBeaconStore((state) => state.load)
  const clearSessions = useBeaconStore((state) => state.clearSessions)
  const scanDevices = useBeaconStore((state) => state.scanDevices)
  const [actionError, setActionError] = useState(null)
  const [scanning, setScanning] = useState(false)

  if (!snapshot) {
    return (
      <Card>
        <CardContent className="py-10 text-center text-sm text-muted-foreground">
          {loading ? '正在连接 agent-beacon…' : error || '暂无数据'}
          {!loading && (
            <div className="mt-4">
              <Button variant="outline" size="sm" onClick={() => load()}>
                重试
              </Button>
            </div>
          )}
        </CardContent>
      </Card>
    )
  }

  const aggregate = snapshot.aggregate
  const effect = EFFECT_MAP[snapshot.effect] ?? EFFECT_MAP.off
  const sessions = snapshot.sessions ?? []
  const devices = snapshot.devices ?? []
  const connectedDevices = devices.filter((device) => device.connected).length

  async function handleClear() {
    try {
      setActionError(null)
      await clearSessions()
    } catch (err) {
      setActionError(err.message)
    }
  }

  async function handleScan() {
    setScanning(true)
    try {
      await scanDevices()
    } catch (err) {
      setActionError(err.message)
    } finally {
      setScanning(false)
    }
  }

  return (
    <div className="space-y-6">
      {actionError && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive-foreground">
          {actionError}
        </div>
      )}

      <div className="grid gap-4 md:grid-cols-3">
        <Card className="md:col-span-2">
          <CardHeader className="flex-row items-start justify-between space-y-0">
            <div>
              <CardDescription>当前聚合状态</CardDescription>
              <CardTitle className="mt-2 text-3xl">
                {AGGREGATE_LABELS[aggregate] ?? aggregate}
              </CardTitle>
            </div>
            <Badge className={cn('border', TONE_CLASSES[effectTone(snapshot.effect)])}>
              <Lightbulb className="mr-1.5 h-3.5 w-3.5" />
              {effect.label}
            </Badge>
          </CardHeader>
          <CardContent className="grid gap-3 text-sm text-muted-foreground sm:grid-cols-3">
            <div className="flex items-center gap-2">
              <Activity className="h-4 w-4" />
              活动会话 {sessions.length}
            </div>
            <div className="flex items-center gap-2">
              <Usb className="h-4 w-4" />
              设备 {connectedDevices}/{devices.length}
            </div>
            <div className="flex items-center gap-2">
              <Clock className="h-4 w-4" />
              运行 {snapshot.service?.uptime_secs ?? 0} 秒
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardDescription>服务</CardDescription>
            <CardTitle className="text-lg">
              {snapshot.service?.version ? `v${snapshot.service.version}` : 'agent-beacon'}
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-1 text-xs text-muted-foreground">
            <div className="flex items-center gap-2">
              <Server className="h-3.5 w-3.5" />
              127.0.0.1:{snapshot.service?.port ?? 6980}
            </div>
            <div>PID {snapshot.service?.pid ?? '-'}</div>
          </CardContent>
        </Card>
      </div>

      {snapshot.override && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-100">
          手动覆盖生效中：{EFFECT_MAP[snapshot.override.effect]?.label} · 到期后自动恢复
        </div>
      )}

      <Card>
        <CardHeader className="flex-row items-center justify-between space-y-0">
          <div>
            <CardTitle className="text-base">活动会话</CardTitle>
            <CardDescription>按最近更新时间排序</CardDescription>
          </div>
          <Dialog>
            <DialogTrigger asChild>
              <Button variant="outline" size="sm" disabled={sessions.length === 0}>
                <Trash2 className="mr-1.5 h-3.5 w-3.5" />
                清空会话
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>清空全部会话？</DialogTitle>
                <DialogDescription>
                  所有 agent 会话会被移除，聚合状态变为「无活动」，灯效按 off 处理。该操作不可撤销。
                </DialogDescription>
              </DialogHeader>
              <DialogFooter>
                <DialogClose asChild>
                  <Button variant="ghost">取消</Button>
                </DialogClose>
                <DialogClose asChild>
                  <Button variant="destructive" onClick={handleClear}>
                    确认清空
                  </Button>
                </DialogClose>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </CardHeader>
        <CardContent>
          {sessions.length === 0 ? (
            <EmptyState
              icon={Activity}
              title="还没有 agent 上报状态"
              description="在 agent 的 hook 中调用 `agent-beacon report`，或向 /api/v1/events 发送事件。"
            />
          ) : (
            <ul className="divide-y divide-border/70">
              {sessions.map((session) => (
                <li
                  key={`${session.agent}:${session.session}`}
                  className="flex flex-wrap items-center gap-3 py-3"
                >
                  <Badge
                    variant="outline"
                    className={cn('border', TONE_CLASSES[STATE_TONE[session.state] ?? 'muted'])}
                  >
                    {STATE_LABELS[session.state] ?? session.state}
                  </Badge>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">{session.agent}</div>
                    <div className="truncate text-xs text-muted-foreground">
                      {session.session}
                      {session.message ? ` · ${session.message}` : ''}
                    </div>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {formatAge(session.updated_at_ms)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex-row items-center justify-between space-y-0">
          <div>
            <CardTitle className="text-base">设备</CardTitle>
            <CardDescription>USB 状态灯与 mock 设备</CardDescription>
          </div>
          <Button variant="outline" size="sm" onClick={handleScan} disabled={scanning}>
            <RefreshCw className={cn('mr-1.5 h-3.5 w-3.5', scanning && 'animate-spin')} />
            {scanning ? '扫描中…' : '重新扫描'}
          </Button>
        </CardHeader>
        <CardContent>
          {devices.length === 0 ? (
            <EmptyState
              icon={Usb}
              title="未发现设备"
              description="插入 MLight 或 YH50 状态灯；没有硬件时可在设置中开启 mock 设备。"
            />
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {devices.map((device) => (
                <div key={device.id} className="rounded-lg border border-border/70 bg-background/40 p-4">
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2 text-sm font-medium">
                      <Usb className="h-4 w-4" />
                      {PROFILE_LABELS[device.profile] ?? device.profile}
                    </div>
                    <Badge variant={device.connected ? 'default' : 'secondary'}>
                      {device.connected ? '已连接' : '未连接'}
                    </Badge>
                  </div>
                  <div className="mt-2 space-y-1 text-xs text-muted-foreground">
                    <div className="truncate">{device.port}</div>
                    <div>当前灯效：{EFFECT_MAP[device.effect]?.label ?? device.effect}</div>
                    {device.last_error && (
                      <div className="text-red-300">错误：{device.last_error}</div>
                    )}
                    {device.mock_commands?.length > 0 && (
                      <div className="truncate font-mono">
                        最近命令：{device.mock_commands[device.mock_commands.length - 1]}
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
