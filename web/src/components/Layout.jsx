import { useEffect } from 'react'
import { NavLink, Outlet } from 'react-router-dom'
import {
  Languages,
  LayoutDashboard,
  Lightbulb,
  Monitor,
  Moon,
  Plug,
  Settings,
  Sun,
  Usb,
} from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { usePreferencesStore } from '../store/preferences'
import { useBeaconStream } from '../hooks/useBeaconStream'
import { useI18n } from '../i18n'
import { cn } from '../lib/utils'
import { Badge } from '../components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from '../components/ui/select'
import BeaconLogo from '../components/BeaconLogo'
import IntegrationOnboarding from '../components/IntegrationOnboarding'
import { resolveLights } from '../lib/lights'

const NAV = [
  { to: '/', labelKey: 'nav.dashboard', icon: LayoutDashboard },
  { to: '/control', labelKey: 'nav.control', icon: Lightbulb },
  { to: '/agents', labelKey: 'nav.agents', icon: Plug },
  { to: '/settings', labelKey: 'nav.settings', icon: Settings },
]

export default function Layout() {
  const snapshot = useBeaconStore((state) => state.snapshot)
  const connected = useBeaconStore((state) => state.connected)
  const load = useBeaconStore((state) => state.load)
  const theme = usePreferencesStore((state) => state.theme)
  const setTheme = usePreferencesStore((state) => state.setTheme)
  const setLanguage = usePreferencesStore((state) => state.setLanguage)
  const { t, language } = useI18n()

  useBeaconStream()

  useEffect(() => {
    if (!snapshot) load()
  }, [snapshot, load])

  const lights = resolveLights(snapshot)
  const connectedDevices = lights.filter((light) => light.connected).length
  const deviceTone =
    !snapshot || lights.length === 0
      ? 'text-muted-foreground'
      : connectedDevices === lights.length
        ? 'text-success'
        : connectedDevices === 0
          ? 'text-danger'
          : 'text-warning'
  const deviceLabel = snapshot
    ? t('header.deviceCount', { connected: connectedDevices, total: lights.length })
    : t('header.deviceNone')
  const deviceTitle = connected ? t('header.deviceTitleLive') : t('header.deviceTitleStale')

  return (
    <div className="min-h-screen">
      <header className="sticky top-0 z-40 border-b border-border/70 bg-background/80 backdrop-blur">
        <div className="mx-auto flex max-w-6xl flex-wrap items-center gap-4 px-4 py-3">
          <div className="flex items-center gap-2.5">
            <BeaconLogo className="h-10 w-10 shrink-0" />
            <div className="leading-tight">
              <div className="text-sm font-semibold tracking-tight">AgentBeacon</div>
              <div className="text-[11px] text-muted-foreground">{t('app.tagline')}</div>
            </div>
          </div>

          <nav className="flex items-center gap-1 rounded-lg border border-border/70 bg-card/60 p-1">
            {NAV.map(({ to, labelKey, icon: Icon }) => (
              <NavLink
                key={to}
                to={to}
                end={to === '/'}
                className={({ isActive }) =>
                  cn(
                    'flex items-center gap-2 rounded-md px-3 py-1.5 text-sm transition-colors',
                    isActive
                      ? 'bg-primary/15 text-primary'
                      : 'text-muted-foreground hover:bg-accent hover:text-foreground',
                  )
                }
              >
                <Icon className="h-4 w-4" />
                {t(labelKey)}
              </NavLink>
            ))}
          </nav>

          <div className="ml-auto flex items-center gap-3">
            <Badge
              variant="outline"
              className={cn('gap-1.5 font-normal', deviceTone)}
              title={deviceTitle}
            >
              <Usb className="h-3.5 w-3.5" />
              {deviceLabel}
            </Badge>
            <Select value={language} onValueChange={setLanguage}>
              <SelectTrigger
                className="h-8 w-auto gap-1 border-input bg-transparent px-2 text-muted-foreground shadow-none hover:bg-accent hover:text-accent-foreground [&>span]:hidden"
                title={t('header.language')}
                aria-label={t('header.language')}
              >
                <Languages className="h-4 w-4 shrink-0" />
              </SelectTrigger>
              <SelectContent className="min-w-0">
                <SelectItem value="zh">{t('header.languageZh')}</SelectItem>
                <SelectItem value="en">{t('header.languageEn')}</SelectItem>
              </SelectContent>
            </Select>
            <Select value={theme} onValueChange={setTheme}>
              <SelectTrigger
                className="h-8 w-auto gap-1 border-input bg-transparent px-2 text-muted-foreground shadow-none hover:bg-accent hover:text-accent-foreground [&>span]:hidden"
                title={t('header.theme')}
                aria-label={t('header.theme')}
              >
                {theme === 'system' ? (
                  <Monitor className="h-4 w-4 shrink-0" />
                ) : theme === 'light' ? (
                  <Sun className="h-4 w-4 shrink-0" />
                ) : (
                  <Moon className="h-4 w-4 shrink-0" />
                )}
              </SelectTrigger>
              <SelectContent className="min-w-0">
                <SelectItem value="system">{t('header.themeSystem')}</SelectItem>
                <SelectItem value="light">{t('header.themeLight')}</SelectItem>
                <SelectItem value="dark">{t('header.themeDark')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-6xl px-4 py-6">
        <Outlet />
      </main>

      <IntegrationOnboarding />
    </div>
  )
}
