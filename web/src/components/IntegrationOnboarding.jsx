import { useEffect, useState } from 'react'
import { useBeaconStore } from '../store/beacon'
import { useI18n } from '../i18n'
import { Button } from '../components/ui/button'
import { Label } from '../components/ui/label'
import { Switch } from '../components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'

/**
 * 首次启动提示：检测到已安装但尚未接入的 agent 时弹一次。
 *
 * 关闭（接入或跳过）后把 `onboarded` 写回配置，之后不再打扰；
 * 用户仍可在“Agent 集成”页面手动安装。
 */
export default function IntegrationOnboarding() {
  const integrations = useBeaconStore((state) => state.integrations)
  const loadIntegrations = useBeaconStore((state) => state.loadIntegrations)
  const saveIntegrationSettings = useBeaconStore((state) => state.saveIntegrationSettings)
  const installAllIntegrations = useBeaconStore((state) => state.installAllIntegrations)
  const { t } = useI18n()
  const [open, setOpen] = useState(false)
  const [autoInstall, setAutoInstall] = useState(false)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (!integrations) loadIntegrations()
  }, [integrations, loadIntegrations])

  useEffect(() => {
    if (!integrations || integrations.onboarded) return
    if ((integrations.pending ?? []).length === 0) return
    setOpen(true)
  }, [integrations])

  const pending = integrations?.pending ?? []
  const names = pending
    .map((id) => integrations?.entries?.find((entry) => entry.id === id)?.label ?? id)
    .join('、')

  async function finish({ install }) {
    setBusy(true)
    try {
      if (install) {
        await saveIntegrationSettings({ auto_install: autoInstall })
        await installAllIntegrations()
      } else {
        await saveIntegrationSettings({ auto_install: autoInstall, onboarded: true })
      }
      setOpen(false)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={() => {}}>
      <DialogContent onInteractOutside={(event) => event.preventDefault()}>
        <DialogHeader>
          <DialogTitle>{t('onboarding.title')}</DialogTitle>
          <DialogDescription>
            {t('onboarding.description', { count: pending.length, agents: names })}
          </DialogDescription>
        </DialogHeader>

        <div className="flex items-center justify-between gap-4 rounded-lg border border-border/70 bg-background/40 px-4 py-3">
          <Label htmlFor="onboarding-auto-install" className="text-sm font-normal">
            {t('onboarding.autoInstall')}
          </Label>
          <Switch
            id="onboarding-auto-install"
            checked={autoInstall}
            onCheckedChange={setAutoInstall}
            disabled={busy}
          />
        </div>

        <p className="text-xs text-muted-foreground">{t('onboarding.footnote')}</p>

        <DialogFooter>
          <Button variant="ghost" onClick={() => finish({ install: false })} disabled={busy}>
            {t('onboarding.skip')}
          </Button>
          <Button onClick={() => finish({ install: true })} disabled={busy}>
            {t('onboarding.installAll')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}