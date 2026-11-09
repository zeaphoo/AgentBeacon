import { useEffect, useState } from 'react'
import { AlertTriangle, Download, Plug, RefreshCw, Trash2 } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useI18n } from '../i18n'
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
import { Label } from '../components/ui/label'
import { Switch } from '../components/ui/switch'
import { cn } from '../lib/utils'

const STATE_TONES = {
  'up-to-date': 'border-success/40 text-success',
  'upgrade-available': 'border-warning/40 text-warning',
  'not-installed': 'text-muted-foreground',
  unmanaged: 'border-danger/40 text-danger',
}

export default function Agents() {
  const integrations = useBeaconStore((state) => state.integrations)
  const loading = useBeaconStore((state) => state.integrationsLoading)
  const loadError = useBeaconStore((state) => state.integrationsError)
  const loadIntegrations = useBeaconStore((state) => state.loadIntegrations)
  const saveIntegrationSettings = useBeaconStore((state) => state.saveIntegrationSettings)
  const installIntegration = useBeaconStore((state) => state.installIntegration)
  const upgradeIntegration = useBeaconStore((state) => state.upgradeIntegration)
  const uninstallIntegration = useBeaconStore((state) => state.uninstallIntegration)
  const installAllIntegrations = useBeaconStore((state) => state.installAllIntegrations)
  const { t } = useI18n()
  const [busy, setBusy] = useState(null)
  const [error, setError] = useState(null)

  useEffect(() => {
    if (!integrations) loadIntegrations()
  }, [integrations, loadIntegrations])

  async function run(key, action) {
    setBusy(key)
    setError(null)
    try {
      await action()
    } catch (err) {
      setError(t('agents.actionFailed', { error: err.message }))
    } finally {
      setBusy(null)
    }
  }

  const entries = integrations?.entries ?? []
  const pending = integrations?.pending ?? []
  // 未检测到的排在后面，避免把没装的 agent 顶在最上面。
  const sorted = [...entries].sort((a, b) => Number(b.detected) - Number(a.detected))

  return (
    <div className="space-y-6">
      {(error || loadError) && (
        <div className="alert-danger rounded-lg border px-4 py-3 text-sm" role="alert">
          {error || t('agents.errorHint', { error: loadError })}
        </div>
      )}

      <Card>
        <CardHeader className="flex-row flex-wrap items-start justify-between gap-3 space-y-0">
          <div>
            <CardTitle className="text-base">{t('agents.title')}</CardTitle>
            <CardDescription>{t('agents.description')}</CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => run('refresh', loadIntegrations)}
              disabled={loading || busy !== null}
            >
              <RefreshCw className={cn('mr-1.5 h-3.5 w-3.5', loading && 'animate-spin')} />
              {t('agents.refresh')}
            </Button>
            <Button
              size="sm"
              onClick={() => run('install-all', installAllIntegrations)}
              disabled={pending.length === 0 || busy !== null}
            >
              <Download className="mr-1.5 h-3.5 w-3.5" />
              {busy === 'install-all' ? t('agents.installing') : t('agents.installAll')}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between gap-4 rounded-lg border border-border/70 bg-background/40 px-4 py-3">
            <div className="space-y-0.5">
              <Label htmlFor="auto-install">{t('agents.autoInstall')}</Label>
              <p className="text-xs text-muted-foreground">{t('agents.autoInstallHint')}</p>
            </div>
            <Switch
              id="auto-install"
              checked={Boolean(integrations?.auto_install)}
              disabled={!integrations || busy !== null}
              onCheckedChange={(checked) =>
                run('auto-install', () => saveIntegrationSettings({ auto_install: checked }))
              }
            />
          </div>

          {integrations && (
            <p className="text-xs text-muted-foreground">
              {t('agents.summary', {
                detected: integrations.detected ?? 0,
                installed: integrations.installed ?? 0,
              })}
            </p>
          )}

          {!integrations && loading ? (
            <p className="py-6 text-center text-sm text-muted-foreground">
              {t('agents.loading')}
            </p>
          ) : sorted.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">{t('agents.empty')}</p>
          ) : (
            <ul className="divide-y divide-border/70">
              {sorted.map((entry) => {
                const installed = entry.installed_version !== null
                const canUpgrade = entry.state === 'upgrade-available'
                return (
                  <li
                    key={entry.id}
                    className="flex flex-wrap items-center gap-3 py-3 first:pt-0 last:pb-0"
                  >
                    <AgentLogo id={entry.id} className="h-6 w-6" />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="text-sm font-medium">{entry.label}</span>
                        <code className="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">
                          {entry.id}
                        </code>
                        <Badge variant="outline" className="font-normal">
                          {t(`agents.kind.${entry.kind}`)}
                        </Badge>
                        <Badge
                          variant="outline"
                          className={cn(
                            'font-normal',
                            entry.detected ? 'border-primary/40 text-primary' : 'text-muted-foreground',
                          )}
                          title={entry.detected ? undefined : t('agents.detectedHint')}
                        >
                          {entry.detected ? t('agents.detected') : t('agents.notDetected')}
                        </Badge>
                        <Badge
                          variant="outline"
                          className={cn('font-normal', STATE_TONES[entry.state])}
                        >
                          {t(`agents.state.${entry.state}`)}
                        </Badge>
                        {installed && (
                          <span className="text-xs text-muted-foreground">
                            {t('agents.version', { version: entry.installed_version })}
                          </span>
                        )}
                      </div>
                      <div className="mt-1 truncate text-xs text-muted-foreground" title={entry.path}>
                        {entry.path}
                      </div>
                      {entry.error && (
                        <div className="mt-1 flex items-center gap-1 text-xs text-danger">
                          <AlertTriangle className="h-3 w-3" />
                          {t('agents.errorHint', { error: entry.error })}
                        </div>
                      )}
                      {entry.state === 'unmanaged' && (
                        <div className="mt-1 text-xs text-warning">
                          {t('agents.unmanagedHint', { path: entry.path })}
                        </div>
                      )}
                    </div>

                    <div className="flex items-center gap-2">
                      {!installed ? (
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={busy !== null || !entry.detected}
                          onClick={() =>
                            run(entry.id, () => installIntegration(entry.id))
                          }
                        >
                          <Plug className="mr-1.5 h-3.5 w-3.5" />
                          {t('agents.install')}
                        </Button>
                      ) : (
                        <>
                          {canUpgrade && (
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={busy !== null}
                              onClick={() =>
                                run(entry.id, () => upgradeIntegration(entry.id))
                              }
                            >
                              <Download className="mr-1.5 h-3.5 w-3.5" />
                              {t('agents.upgrade')}
                            </Button>
                          )}
                          <Button
                            variant="ghost"
                            size="sm"
                            disabled={busy !== null}
                            onClick={() =>
                              run(entry.id, () => uninstallIntegration(entry.id))
                            }
                          >
                            <Trash2 className="mr-1.5 h-3.5 w-3.5" />
                            {t('agents.uninstall')}
                          </Button>
                        </>
                      )}
                    </div>
                  </li>
                )
              })}
            </ul>
          )}
        </CardContent>
      </Card>
    </div>
  )
}