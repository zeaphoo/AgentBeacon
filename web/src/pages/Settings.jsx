import { useEffect, useRef, useState } from 'react'
import { AlertTriangle, RefreshCw, Server, Usb } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useI18n } from '../i18n'
import { AGGREGATE_KEYS, EFFECTS, profileLabel } from '../lib/labels'
import AgentLogo from '../components/AgentLogo'
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
import { cn } from '../lib/utils'

const MAPPING_ROWS = ['none', 'idle', 'working', 'waiting', 'error']
const STRATEGIES = ['unified', 'auto_agent', 'manual_agent', 'auto_session']

function clone(value) {
  return JSON.parse(JSON.stringify(value))
}

export default function Settings() {
  const snapshot = useBeaconStore((state) => state.snapshot)
  const saveConfig = useBeaconStore((state) => state.saveConfig)
  const rebuildRouting = useBeaconStore((state) => state.rebuildRouting)
  const integrations = useBeaconStore((state) => state.integrations)
  const loadIntegrations = useBeaconStore((state) => state.loadIntegrations)
  const { t } = useI18n()
  const [form, setForm] = useState(null)
  const [saving, setSaving] = useState(false)
  const [status, setStatus] = useState(null)
  const [error, setError] = useState(null)
  const [buzzerDialogOpen, setBuzzerDialogOpen] = useState(false)
  const [rebuilding, setRebuilding] = useState(false)
  const [routingStatus, setRoutingStatus] = useState(null)
  const savedRef = useRef(null)
  const requestRef = useRef(0)
  // integrations 由「Agent 集成」页面与本页共写同一份配置；保存时始终采用
  // 服务端最新值，避免本页的旧副本把自动安装开关改回去。
  const integrationsRef = useRef(null)

  useEffect(() => {
    if (snapshot?.config?.integrations) {
      integrationsRef.current = snapshot.config.integrations
    }
  }, [snapshot])

  useEffect(() => {
    if (!integrations) loadIntegrations()
  }, [integrations, loadIntegrations])

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

  function update(patch) {
    setForm((current) => ({ ...current, ...patch }))
  }

  const strategy = form.routing?.strategy ?? 'unified'
  const manual = form.routing?.manual ?? {}
  const runtimeById = new Map((snapshot?.devices ?? []).map((item) => [item.id, item]))
  const routes = (
    snapshot?.routing?.length
      ? snapshot.routing
      : (snapshot?.devices ?? []).map((item) => ({
          device: item.id,
          label: item.label,
          connected: item.connected,
        }))
  ).filter((route) => runtimeById.get(route.device)?.profile !== 'mock')
  const routeById = new Map(routes.map((route) => [route.device, route]))
  const detectedAgents = (integrations?.entries ?? [])
    .filter((entry) => entry.detected)
    .map((entry) => entry.id)
  const activeAgents = [
    ...new Set([
      ...(snapshot?.sessions ?? []).map((session) => session.agent),
      ...Object.values(manual).flat(),
      ...detectedAgents,
    ]),
  ].sort()
  const assignedAgents = new Set(Object.values(manual).flat())
  const manualUnassigned = activeAgents.filter((agent) => !assignedAgents.has(agent))
  const snapshotUnassigned = snapshot?.unassigned_agents ?? []
  const lights = [
    ...routes
      .filter(
        (route) => route.connected || (manual[route.device]?.length ?? 0) > 0,
      )
      .map((route) => ({
        device: route.device,
        label: route.label,
        connected: route.connected,
        stale: false,
      })),
    ...Object.keys(manual)
      .filter((device) => !routeById.has(device))
      .map((device) => ({
        device,
        label: device,
        connected: false,
        stale: true,
      })),
  ].sort((a, b) => (a.label ?? a.device).localeCompare(b.label ?? b.device))

  function bindingForDevice(device) {
    const runtime = runtimeById.get(device)
    const serial = device.startsWith('serial:') ? device.slice('serial:'.length) : null
    const port = device.startsWith('port:') ? device.slice('port:'.length) : runtime?.port
    return (form.devices ?? []).find(
      (binding) =>
        binding.id === device ||
        (serial && binding.serial === serial) ||
        (port && binding.port === port) ||
        binding.port === device,
    )
  }

  function profileForDevice(device) {
    const runtime = runtimeById.get(device)
    return bindingForDevice(device)?.profile ?? runtime?.profile ?? null
  }

  function agentLogoId(agent) {
    const value = agent.toLowerCase()
    if (value.includes('claude')) return 'claude-code'
    if (value.includes('codex')) return 'codex'
    if (value.includes('gemini')) return 'gemini'
    if (value.includes('qwen')) return 'qwen'
    if (value.includes('iflow')) return 'iflow'
    if (value.includes('qoder')) return 'qoder'
    if (value.includes('opencode')) return 'opencode'
    if (value.includes('workbuddy') || value.includes('work buddy')) return 'workbuddy'
    if (value.includes('codebuddy') || value.includes('code buddy')) return 'codebuddy'
    if (value.includes('copilot')) return 'github-copilot'
    if (value.includes('hermes')) return 'hermes'
    if (value.includes('zcode') || value.includes('z-code')) return 'zcode'
    if (value.includes('antigravity')) return 'antigravity'
    if (value.includes('grok')) return 'grok'
    if (value === 'pi') return 'pi'
    if (value.includes('dsh')) return 'dsh'
    return agent
  }

  function setStrategy(next) {
    setForm((current) => ({
      ...current,
      routing: {
        ...(current.routing ?? { manual: {} }),
        strategy: next,
      },
    }))
  }

  function addAgent(device, agent) {
    if (!agent) return
    setForm((current) => {
      const next = {}
      for (const [key, agents] of Object.entries(current.routing?.manual ?? {})) {
        const remaining = agents.filter((item) => item !== agent)
        if (remaining.length > 0) next[key] = remaining
      }
      next[device] = [...(next[device] ?? []), agent]
      return {
        ...current,
        routing: {
          ...(current.routing ?? { strategy: 'unified' }),
          manual: next,
        },
      }
    })
  }

  function removeAgent(device, agent) {
    setForm((current) => {
      const next = { ...(current.routing?.manual ?? {}) }
      const remaining = (next[device] ?? []).filter((item) => item !== agent)
      if (remaining.length > 0) next[device] = remaining
      else delete next[device]
      return {
        ...current,
        routing: {
          ...(current.routing ?? { strategy: 'unified' }),
          manual: next,
        },
      }
    })
  }

  function deleteLight(device) {
    setForm((current) => {
      const next = { ...(current.routing?.manual ?? {}) }
      delete next[device]
      const devices = (current.devices ?? []).filter(
        (binding) => !bindingMatchesDevice(binding, device),
      )
      return {
        ...current,
        devices,
        routing: {
          ...(current.routing ?? { strategy: 'unified' }),
          manual: next,
        },
      }
    })
  }

  function bindingMatchesDevice(binding, device) {
    if (binding.id === device) return true
    if (device.startsWith('serial:') && binding.serial === device.slice(7)) return true
    if (device.startsWith('port:') && binding.port === device.slice(5)) return true
    return binding.port === device
  }

  async function handleRebuild() {
    setRebuilding(true)
    setRoutingStatus(null)
    setError(null)
    try {
      await rebuildRouting()
      setRoutingStatus('rebuilt')
    } catch (err) {
      setRoutingStatus(err.message)
      setError(err.message)
    } finally {
      setRebuilding(false)
    }
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
          <CardTitle className="text-base">{t('settings.routingTitle')}</CardTitle>
          <CardDescription>{t('settings.routingDescription')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-5">
          <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
            {STRATEGIES.map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => setStrategy(option)}
                aria-pressed={strategy === option}
                className={cn(
                  'rounded-lg border px-3 py-3 text-left transition-colors',
                  strategy === option
                    ? 'border-primary bg-primary/5 ring-1 ring-primary'
                    : 'border-border/70 hover:bg-muted/50',
                )}
              >
                <div className="text-sm font-medium">{t(`strategy.${option}`)}</div>
                <div className="mt-1 text-xs text-muted-foreground">
                  {t(`strategy.${option}.description`)}
                </div>
              </button>
            ))}
          </div>

          {strategy === 'manual_agent' ? (
            <div className="space-y-3 border-t border-border/70 pt-4">
              <div>
                <div className="text-sm font-medium">{t('settings.manualTitle')}</div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t('settings.manualHint')}
                </p>
                {activeAgents.length === 0 && (
                  <p className="mt-1 text-xs text-warning">
                    {t('settings.noAgentsDetected')}
                  </p>
                )}
              </div>
              {lights.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t('settings.noDevices')}</p>
              ) : (
                <ul className="space-y-3">
                  {lights.map((light) => {
                    const agents = manual[light.device] ?? []
                    const profile = profileForDevice(light.device)
                    const available = activeAgents.filter(
                      (agent) => !agents.includes(agent),
                    )
                    return (
                      <li
                        key={light.device}
                        className="rounded-lg border border-border/70 p-3"
                      >
                        <div className="flex flex-wrap items-start justify-between gap-4">
                          <div className="flex min-w-0 items-start gap-3">
                            <Usb className="mt-1 h-4 w-4 shrink-0 text-muted-foreground" />
                            <div className="min-w-0">
                              <div className="flex flex-wrap items-center gap-2">
                                <span className="text-sm font-medium">
                                  {light.label ?? light.device}
                                </span>
                                <Badge
                                  variant={light.connected ? 'default' : 'secondary'}
                                >
                                  {light.connected
                                    ? t('settings.online')
                                    : light.stale
                                      ? t('settings.staleLight')
                                      : t('settings.offline')}
                                </Badge>
                              </div>
                              <div className="mt-0.5 text-xs text-muted-foreground">
                                {profile
                                  ? profileLabel(profile, t)
                                  : t('settings.unknownProfile')}
                                <span className="mx-1">·</span>
                                <span className="font-mono">
                                  {runtimeById.get(light.device)?.port ?? light.device}
                                </span>
                              </div>
                            </div>
                          </div>

                          <div className="flex max-w-full flex-col items-end gap-2">
                            <div className="flex flex-wrap items-center justify-end gap-2">
                              {agents.length === 0 ? (
                                <span className="text-xs text-muted-foreground">
                                  {t('settings.noAssignedAgents')}
                                </span>
                              ) : (
                                agents.map((agent) => (
                                  <span
                                    key={agent}
                                    className="inline-flex items-center gap-1.5 rounded-full border border-border/70 px-2 py-1 text-xs"
                                  >
                                    <AgentLogo
                                      id={agentLogoId(agent)}
                                      className="h-4 w-4"
                                    />
                                    {agent}
                                    <button
                                      type="button"
                                      className="text-muted-foreground hover:text-foreground"
                                      aria-label={`${t('settings.removeAgent')} ${agent}`}
                                      onClick={() => removeAgent(light.device, agent)}
                                    >
                                      ×
                                    </button>
                                  </span>
                                ))
                              )}
                              <Select
                                value=""
                                onValueChange={(agent) => addAgent(light.device, agent)}
                                disabled={available.length === 0}
                              >
                                <SelectTrigger
                                  className="h-8 w-40"
                                  aria-label={`${t('settings.addAgent')} · ${light.label ?? light.device}`}
                                >
                                  <SelectValue placeholder={t('settings.addAgent')} />
                                </SelectTrigger>
                                <SelectContent>
                                  {available.map((agent) => (
                                    <SelectItem key={agent} value={agent}>
                                      <span className="flex items-center gap-2">
                                        <AgentLogo
                                          id={agentLogoId(agent)}
                                          className="h-4 w-4"
                                        />
                                        {agent}
                                      </span>
                                    </SelectItem>
                                  ))}
                                </SelectContent>
                              </Select>
                            </div>
                            {!light.connected && (
                              <Button
                                variant="ghost"
                                size="sm"
                                onClick={() => deleteLight(light.device)}
                              >
                                {t('settings.deleteLight')}
                              </Button>
                            )}
                          </div>
                        </div>
                      </li>
                    )
                  })}
                </ul>
              )}
              {manualUnassigned.length > 0 && (
                <p className="text-xs text-warning">
                  {t('settings.unassignedAgents', {
                    agents: manualUnassigned.join('、'),
                  })}
                </p>
              )}
            </div>
          ) : (
            <div className="space-y-2 border-t border-border/70 pt-4">
              <p className="text-xs text-muted-foreground">
                {t(`strategy.${strategy}.hint`)}
              </p>
              {(strategy === 'auto_agent' || strategy === 'auto_session') && (
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleRebuild}
                    disabled={rebuilding}
                  >
                    <RefreshCw
                      className={cn('mr-1.5 h-3.5 w-3.5', rebuilding && 'animate-spin')}
                    />
                    {rebuilding ? t('settings.rebuilding') : t('settings.rebuild')}
                  </Button>
                  {routingStatus === 'rebuilt' && (
                    <span className="text-xs text-muted-foreground">
                      {t('settings.rebuilt')}
                    </span>
                  )}
                  {routingStatus && routingStatus !== 'rebuilt' && (
                    <span className="text-xs text-danger">{routingStatus}</span>
                  )}
                </div>
              )}
              {snapshotUnassigned.length > 0 && (
                <p className="text-xs text-warning">
                  {t('settings.unassignedAgents', {
                    agents: snapshotUnassigned.join('、'),
                  })}
                </p>
              )}
            </div>
          )}
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
