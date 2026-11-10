import { useEffect, useState } from 'react'
import {
  Activity,
  AlertTriangle,
  CircleSlash,
  Clock,
  Lightbulb,
  Moon,
  RefreshCw,
  Trash2,
  Usb,
} from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useI18n } from '../i18n'
import {
  TONE_CLASSES,
  aggregateLabel,
  capabilitiesForProfile,
  connectionLabel,
  effectLabel,
  effectTone,
  formatAge,
  formatUptime,
  profileLabel,
  stateLabel,
  stateTone,
} from '../lib/labels'
import AgentLogo from '../components/AgentLogo'
import DeviceFeatures from '../components/DeviceFeatures'
import { resolveLights } from '../lib/lights'
import { agentLogoId } from '../lib/agents'
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

const AGGREGATE_ICONS = {
  none: CircleSlash,
  idle: Moon,
  working: Activity,
  waiting: Clock,
  error: AlertTriangle,
}

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
  const [now, setNow] = useState(Date.now())
  const { t, language } = useI18n()

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000)
    return () => clearInterval(timer)
  }, [])

  if (!snapshot) {
    return (
      <Card>
        <CardContent className="py-10 text-center text-sm text-muted-foreground">
          {loading ? t('dashboard.connecting') : error || t('dashboard.noData')}
          {!loading && (
            <div className="mt-4">
              <Button variant="outline" size="sm" onClick={() => load()}>
                {t('dashboard.retry')}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>
    )
  }

  const uptimeSeconds = snapshot.service?.started_at_ms
    ? Math.max(0, Math.floor((now - snapshot.service.started_at_ms) / 1000))
    : (snapshot.service?.uptime_secs ?? 0)
  const aggregate = snapshot.aggregate
  const aggregateState = AGGREGATE_ICONS[aggregate] ? aggregate : 'none'
  const AggregateIcon = AGGREGATE_ICONS[aggregateState]
  const sessions = snapshot.sessions ?? []
  const devices = snapshot.devices ?? []
  const runtimeById = new Map(devices.map((device) => [device.id, device]))
  const lights = resolveLights(snapshot)
  const connectedDevices = lights.filter((light) => light.connected).length

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
        <div className="alert-danger rounded-lg border px-4 py-3 text-sm">
          {actionError}
        </div>
      )}

      <Card
        className={cn(
          'aggregate-card',
          `aggregate-${aggregateState}`,
        )}
      >
        <CardHeader className="flex-row flex-wrap items-start justify-between gap-3 space-y-0">
          <div className="flex items-start gap-4">
            <div className="aggregate-icon flex h-14 w-14 shrink-0 items-center justify-center rounded-xl border">
              <AggregateIcon className="h-7 w-7" />
            </div>
            <div>
              <CardDescription>{t('dashboard.aggregateTitle')}</CardDescription>
              <CardTitle className="mt-1 text-3xl">
                {aggregateLabel(aggregate, t)}
              </CardTitle>
            </div>
          </div>
          <Badge className={cn('border', TONE_CLASSES[effectTone(snapshot.effect)])}>
            <Lightbulb className="mr-1.5 h-3.5 w-3.5" />
            {effectLabel(snapshot.effect, t)}
          </Badge>
        </CardHeader>
        <CardContent className="grid gap-3 pt-4 text-sm text-muted-foreground sm:grid-cols-3">
          <div className="flex items-center gap-2">
            <Activity className="h-4 w-4" />
            {t('dashboard.activeSessions')} {sessions.length}
          </div>
          <div className="flex items-center gap-2">
            <Usb className="h-4 w-4" />
            {t('dashboard.devicesSummary', {
              connected: connectedDevices,
              total: lights.length,
            })}
          </div>
          <div className="flex items-center gap-2">
            <Clock className="h-4 w-4" />
            {t('dashboard.uptime', {
              duration: formatUptime(uptimeSeconds, t),
            })}
          </div>
        </CardContent>
      </Card>

      {snapshot.override && (
        <div className="alert-warning rounded-lg border px-4 py-3 text-sm">
          {t('dashboard.overrideActive', {
            effect: effectLabel(snapshot.override.effect, t),
          })}
        </div>
      )}

      <Card>
        <CardHeader className="flex-row items-center justify-between space-y-0">
          <div>
            <CardTitle className="text-base">{t('dashboard.sessionsTitle')}</CardTitle>
            <CardDescription>{t('dashboard.sessionsDescription')}</CardDescription>
          </div>
          <Dialog>
            <DialogTrigger asChild>
              <Button variant="outline" size="sm" disabled={sessions.length === 0}>
                <Trash2 className="mr-1.5 h-3.5 w-3.5" />
                {t('dashboard.clearSessions')}
              </Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t('dashboard.clearTitle')}</DialogTitle>
                <DialogDescription>{t('dashboard.clearDescription')}</DialogDescription>
              </DialogHeader>
              <DialogFooter>
                <DialogClose asChild>
                  <Button variant="ghost">{t('dashboard.cancel')}</Button>
                </DialogClose>
                <DialogClose asChild>
                  <Button variant="destructive" onClick={handleClear}>
                    {t('dashboard.confirmClear')}
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
              title={t('dashboard.noSessionsTitle')}
              description={t('dashboard.noSessionsDescription')}
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
                    className={cn(
                      'border',
                      TONE_CLASSES[stateTone(session.state, snapshot.config?.mapping)],
                    )}
                  >
                    {stateLabel(session.state, t)}
                  </Badge>
                  <AgentLogo
                    id={agentLogoId(session.agent)}
                    className="h-5 w-5"
                  />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">{session.agent}</div>
                    <div className="truncate text-xs text-muted-foreground">
                      {session.session}
                      {session.message ? ` · ${session.message}` : ''}
                    </div>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {formatAge(session.updated_at_ms, language)}
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
            <CardTitle className="text-base">{t('dashboard.devicesTitle')}</CardTitle>
            <CardDescription>{t('dashboard.devicesDescription')}</CardDescription>
          </div>
          <Button variant="outline" size="sm" onClick={handleScan} disabled={scanning}>
            <RefreshCw className={cn('mr-1.5 h-3.5 w-3.5', scanning && 'animate-spin')} />
            {scanning ? t('dashboard.scanning') : t('dashboard.scan')}
          </Button>
        </CardHeader>
        <CardContent>
          {lights.length === 0 ? (
            <EmptyState
              icon={Usb}
              title={t('dashboard.noDevicesTitle')}
              description={t('dashboard.noDevicesDescription')}
            />
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {lights.map((light) => {
                const runtime = runtimeById.get(light.device)
                const state = AGGREGATE_ICONS[light.aggregate] ? light.aggregate : 'none'
                const StateIcon = AGGREGATE_ICONS[state]
                const effect = light.effect ?? runtime?.effect ?? 'off'
                return (
                  <div
                    key={light.device}
                    data-testid={`device-${light.device}`}
                    className={cn('aggregate-card rounded-lg border p-4', `aggregate-${state}`)}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div className="flex min-w-0 items-start gap-3">
                        <div className="aggregate-icon flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border">
                          <StateIcon className="h-5 w-5" />
                        </div>
                        <div className="min-w-0">
                          <div className="truncate text-xs text-muted-foreground">
                            {light.label ??
                              (runtime
                                ? profileLabel(runtime.profile, t)
                                : light.device)}
                          </div>
                          <div className="text-lg font-semibold">
                            {aggregateLabel(light.aggregate, t)}
                          </div>
                        </div>
                      </div>
                      <Badge variant={light.connected ? 'default' : 'secondary'}>
                        {light.connected
                          ? t('dashboard.connected')
                          : t('dashboard.disconnected')}
                      </Badge>
                    </div>

                    <div className="mt-3 flex flex-wrap items-center gap-2">
                      <Badge className={cn('border', TONE_CLASSES[effectTone(effect)])}>
                        <Lightbulb className="mr-1 h-3 w-3" />
                        {effectLabel(effect, t)}
                      </Badge>
                      {light.overridden && (
                        <Badge variant="outline">{t('dashboard.overridden')}</Badge>
                      )}
                    </div>

                    <div className="mt-3 space-y-1 text-xs text-muted-foreground">
                      <div>
                        {t(
                          ['auto_agent', 'manual_agent'].includes(
                            snapshot.config?.routing?.strategy,
                          )
                            ? 'dashboard.assignedAgents'
                            : 'dashboard.showingAgents',
                          {
                            agents: light.agents?.length
                              ? light.agents.join(', ')
                              : t('dashboard.noAgents'),
                          },
                        )}
                      </div>
                      {runtime ? (
                        <>
                          <div className="truncate">
                            {connectionLabel(runtime.port, t)}
                            <span className="mx-1">·</span>
                            <span className="font-mono">{runtime.port}</span>
                          </div>
                          <DeviceFeatures
                            capabilities={
                              runtime.capabilities ??
                              capabilitiesForProfile(runtime.profile, snapshot.profiles)
                            }
                            className="mt-1"
                          />
                        </>
                      ) : (
                        <div className="truncate">
                          {connectionLabel(light.device, t)}
                          <span className="mx-1">·</span>
                          <span className="font-mono">{light.device}</span>
                        </div>
                      )}
                      {runtime?.last_error && (
                        <div className="text-danger">
                          {t('dashboard.deviceError', { error: runtime.last_error })}
                        </div>
                      )}
                    </div>
                  </div>
                )
              })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
