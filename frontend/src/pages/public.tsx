// 公开展示页：匿名可见（脱敏：无 IP/系统信息），不要求登录
import { useEffect, useState } from 'react'
import { Card, CardContent } from '@/components/ui/card'
import { ThemeToggle } from '@/components/theme-toggle'
import { fmtPercent, fmtSpeed, fmtUptime, usageColor } from '@/lib/format'
import { cn } from '@/lib/utils'

interface PublicServer {
  name: string
  online: boolean
  cpu_usage: number | null
  mem_pct: number | null
  net_in_speed: number | null
  net_out_speed: number | null
  uptime_secs: number | null
}

export function PublicPage() {
  const [servers, setServers] = useState<PublicServer[]>([])
  useEffect(() => {
    const load = () =>
      fetch('/api/public/servers').then((r) => r.json()).then((d) => setServers(d.servers ?? []))
    load()
    const t = setInterval(load, 5000)
    return () => clearInterval(t)
  }, [])

  return (
    <div className="min-h-svh bg-background text-foreground">
      <header className="border-b">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-4 py-3">
          <span className="text-lg font-semibold tracking-tight">zhengtang</span>
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground">公开状态页 · 5s 自动刷新</span>
            <ThemeToggle />
          </div>
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-4 py-6">
        {servers.length === 0 ? (
          <div className="py-20 text-center text-muted-foreground">暂无公开展示的服务器</div>
        ) : (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {servers.map((s) => (
              <Card key={s.name} className={cn('py-3', !s.online && 'opacity-60')}>
                <CardContent className="space-y-2 px-4">
                  <div className="flex items-center justify-between">
                    <div className="flex min-w-0 items-center gap-2">
                      <span className={cn('size-2 rounded-full', s.online ? 'bg-emerald-500 animate-pulse' : 'bg-muted-foreground/40')} />
                      <span className="truncate text-sm font-medium">{s.name}</span>
                    </div>
                    <span className="text-xs text-muted-foreground">
                      {s.online && s.uptime_secs ? fmtUptime(s.uptime_secs) : '离线'}
                    </span>
                  </div>
                  <div className="flex items-center justify-between text-xs text-muted-foreground">
                    <span>
                      CPU <span className={cn('font-semibold tabular-nums', s.cpu_usage != null ? usageColor(s.cpu_usage) : '')}>
                        {s.cpu_usage != null ? fmtPercent(s.cpu_usage, 0) : '—'}
                      </span>
                    </span>
                    <span>
                      内存 <span className={cn('font-semibold tabular-nums', s.mem_pct != null ? usageColor(s.mem_pct) : '')}>
                        {s.mem_pct != null ? fmtPercent(s.mem_pct, 0) : '—'}
                      </span>
                    </span>
                    <span className="tabular-nums">
                      {s.net_in_speed != null ? `↓${fmtSpeed(s.net_in_speed)} ↑${fmtSpeed(s.net_out_speed ?? 0)}` : '—'}
                    </span>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        )}
      </main>
    </div>
  )
}
