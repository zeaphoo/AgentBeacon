import { useEffect } from 'react'
import { NavLink, Outlet } from 'react-router-dom'
import { Activity, Lightbulb, Settings, Wifi, WifiOff } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useBeaconStream } from '../hooks/useBeaconStream'
import { cn } from '../lib/utils'
import { Badge } from '../components/ui/badge'

const NAV = [
  { to: '/', label: '仪表盘', icon: Activity },
  { to: '/control', label: '手动控制', icon: Lightbulb },
  { to: '/settings', label: '设置', icon: Settings },
]

export default function Layout() {
  const snapshot = useBeaconStore((state) => state.snapshot)
  const connected = useBeaconStore((state) => state.connected)
  const load = useBeaconStore((state) => state.load)

  useBeaconStream()

  useEffect(() => {
    if (!snapshot) load()
  }, [snapshot, load])

  return (
    <div className="min-h-screen">
      <header className="sticky top-0 z-40 border-b border-border/70 bg-background/80 backdrop-blur">
        <div className="mx-auto flex max-w-6xl flex-wrap items-center gap-4 px-4 py-3">
          <div className="flex items-center gap-2">
            <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary/15 text-primary">
              <Activity className="h-4 w-4" />
            </span>
            <div className="leading-tight">
              <div className="text-sm font-semibold tracking-tight">AgentBeacon</div>
              <div className="text-[11px] text-muted-foreground">agent 状态 · USB 状态灯</div>
            </div>
          </div>

          <nav className="flex items-center gap-1 rounded-lg border border-border/70 bg-card/60 p-1">
            {NAV.map(({ to, label, icon: Icon }) => (
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
                {label}
              </NavLink>
            ))}
          </nav>

          <div className="ml-auto flex items-center gap-3">
            <Badge variant="outline" className="gap-1.5 font-normal">
              {connected ? (
                <Wifi className="h-3.5 w-3.5 text-emerald-400" />
              ) : (
                <WifiOff className="h-3.5 w-3.5 text-red-400" />
              )}
              {connected ? '实时连接' : '连接断开'}
            </Badge>
            {snapshot?.service?.version && (
              <span className="text-xs text-muted-foreground">v{snapshot.service.version}</span>
            )}
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-6xl px-4 py-6">
        <Outlet />
      </main>
    </div>
  )
}
