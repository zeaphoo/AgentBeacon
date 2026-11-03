import { useEffect } from 'react'
import { NavLink, Outlet } from 'react-router-dom'
import { LayoutDashboard, Lightbulb, Settings, Usb } from 'lucide-react'
import { useBeaconStore } from '../store/beacon'
import { useBeaconStream } from '../hooks/useBeaconStream'
import { cn } from '../lib/utils'
import { Badge } from '../components/ui/badge'
import BeaconLogo from '../components/BeaconLogo'

const NAV = [
  { to: '/', label: '仪表盘', icon: LayoutDashboard },
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

  const devices = snapshot?.devices ?? []
  const connectedDevices = devices.filter((device) => device.connected).length
  const deviceTone =
    !snapshot || devices.length === 0
      ? 'text-muted-foreground'
      : connectedDevices === devices.length
        ? 'text-emerald-400'
        : connectedDevices === 0
          ? 'text-red-400'
          : 'text-amber-400'
  const deviceLabel = snapshot ? `设备 ${connectedDevices}/${devices.length}` : '设备 —'
  const deviceTitle = connected
    ? 'USB 状态灯与 mock 设备连接数，正在实时更新'
    : '实时数据已中断，设备状态可能不是最新'

  return (
    <div className="min-h-screen">
      <header className="sticky top-0 z-40 border-b border-border/70 bg-background/80 backdrop-blur">
        <div className="mx-auto flex max-w-6xl flex-wrap items-center gap-4 px-4 py-3">
          <div className="flex items-center gap-2.5">
            <BeaconLogo className="h-10 w-10 shrink-0" />
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
            <Badge
              variant="outline"
              className={cn('gap-1.5 font-normal', deviceTone)}
              title={deviceTitle}
            >
              <Usb className="h-3.5 w-3.5" />
              {deviceLabel}
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
