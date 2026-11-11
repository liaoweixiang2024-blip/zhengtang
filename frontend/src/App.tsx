// 路由：#/ 仪表盘，#/server/{id} 详情（hash 路由，零依赖）
import { useEffect, useMemo, useState } from 'react'
import { GroupTabs, StatCards, ViewModeToggle, type ViewMode } from '@/components/dashboard-header'
import { ServerCard, ServerCardMini } from '@/components/server-card'
import { ServerTable } from '@/components/server-table'
import { ThemeToggle } from '@/components/theme-toggle'
import { ServerDetail } from '@/pages/server-detail'
import { useServers } from '@/hooks/useServers'
import { cn } from '@/lib/utils'

const VIEW_KEY = 'zt-view-mode'

function useHashRoute(): [string, (h: string) => void] {
  const [hash, setHash] = useState(() => location.hash || '#/')
  useEffect(() => {
    const on = () => setHash(location.hash || '#/')
    addEventListener('hashchange', on)
    return () => removeEventListener('hashchange', on)
  }, [])
  return [hash, (h) => {
    location.hash = h
  }]
}

export default function App() {
  const { servers, status } = useServers()
  const [hash, navigate] = useHashRoute()
  const [view, setView] = useState<ViewMode>(
    () => (localStorage.getItem(VIEW_KEY) as ViewMode) || 'detailed',
  )
  const [group, setGroup] = useState<number | 'all'>('all')

  const changeView = (v: ViewMode) => {
    setView(v)
    localStorage.setItem(VIEW_KEY, v)
  }

  const list = useMemo(() => [...servers.values()], [servers])

  // 详情页
  const detailId = hash.startsWith('#/server/') ? hash.slice('#/server/'.length) : null
  const detailServer = detailId ? servers.get(detailId) : null
  if (detailId) {
    return (
      <div className="min-h-svh bg-background text-foreground">
        <main className="mx-auto max-w-6xl px-4 py-6">
          {detailServer ? (
            <ServerDetail server={detailServer} onBack={() => navigate('#/')} />
          ) : (
            <div className="py-20 text-center text-muted-foreground">加载中…</div>
          )}
        </main>
      </div>
    )
  }

  // 分组（组名暂用 id 显示，M4 管理页接入命名）
  const groups = useMemo(() => {
    const map = new Map<number, number>()
    for (const s of list) if (s.group_id != null) map.set(s.group_id, (map.get(s.group_id) ?? 0) + 1)
    return [...map.entries()].map(([id, count]) => ({ id, name: `分组 ${id}`, count }))
  }, [list])

  const filtered = group === 'all' ? list : list.filter((s) => s.group_id === group)
  // 在线优先，其次名称
  const sorted = [...filtered].sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name))

  return (
    <div className="min-h-svh bg-background text-foreground">
      {/* 顶栏 */}
      <header className="sticky top-0 z-10 border-b bg-background/80 backdrop-blur">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-4 py-3">
          <div className="flex items-center gap-2">
            <span className="text-lg font-semibold tracking-tight">zhengtang</span>
            <span
              className={cn(
                'size-2 rounded-full',
                status === 'open' ? 'bg-emerald-500' : status === 'connecting' ? 'bg-amber-500 animate-pulse' : 'bg-red-500',
              )}
              title={status === 'open' ? '实时连接正常' : status === 'connecting' ? '连接中…' : '已断开，自动重连中'}
            />
          </div>
          <div className="flex items-center gap-2">
            <ViewModeToggle value={view} onChange={changeView} />
            <ThemeToggle />
          </div>
        </div>
      </header>

      <main className="mx-auto max-w-6xl space-y-5 px-4 py-6">
        <StatCards servers={list} />

        {groups.length > 0 && <GroupTabs groups={groups} active={group} onChange={setGroup} />}

        {sorted.length === 0 ? (
          <div className="py-20 text-center text-muted-foreground">
            {list.length === 0 ? '还没有服务器，用 zhengtang-server add-server 添加一台' : '该分组暂无服务器'}
          </div>
        ) : view === 'compact' ? (
          <ServerTable servers={sorted} onOpen={(id) => navigate(`#/server/${id}`)} />
        ) : view === 'mini' ? (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {sorted.map((s) => (
              <button key={s.id} className="text-left" onClick={() => navigate(`#/server/${s.id}`)}>
                <ServerCardMini server={s} />
              </button>
            ))}
          </div>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
            {sorted.map((s) => (
              <button key={s.id} className="text-left" onClick={() => navigate(`#/server/${s.id}`)}>
                <ServerCard server={s} />
              </button>
            ))}
          </div>
        )}
      </main>

      <footer className="pb-8 text-center text-xs text-muted-foreground">
        zhengtang · 严格只读探针
      </footer>
    </div>
  )
}
