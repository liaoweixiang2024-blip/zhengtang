// 路由：#/ 仪表盘 | #/server/{id} 详情 | #/login 登录/向导 | #/admin 管理
import { useCallback, useEffect, useMemo, useState } from 'react'
import { GroupTabs, StatCards, ViewModeToggle, type ViewMode } from '@/components/dashboard-header'
import { ServerCard, ServerCardMini } from '@/components/server-card'
import { ServerTable } from '@/components/server-table'
import { ThemeToggle } from '@/components/theme-toggle'
import { Login } from '@/pages/login'
import { Admin } from '@/pages/admin'
import { ServerDetail } from '@/pages/server-detail'
import { useServers } from '@/hooks/useServers'
import { cn } from '@/lib/utils'
import { LogOut, Settings } from 'lucide-react'

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

interface Me { username: string; role: string; group_ids: number[] }

export default function App() {
  const [hash, navigate] = useHashRoute()
  const [me, setMe] = useState<Me | null | undefined>(undefined) // undefined=加载中
  const [setupNeeded, setSetupNeeded] = useState(false)
  const [view, setView] = useState<ViewMode>(
    () => (localStorage.getItem(VIEW_KEY) as ViewMode) || 'detailed',
  )
  const [group, setGroup] = useState<number | 'all'>('all')

  const loadMe = useCallback(() => {
    fetch('/api/auth/me')
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => setMe(d))
      .catch(() => setMe(null))
  }, [])
  useEffect(loadMe, [loadMe])

  useEffect(() => {
    fetch('/api/status').then((r) => r.json()).then((d) => setSetupNeeded(!!d.setup_needed))
  }, [])

  const changeView = (v: ViewMode) => {
    setView(v)
    localStorage.setItem(VIEW_KEY, v)
  }

  const logout = async () => {
    await fetch('/api/auth/logout', { method: 'POST' })
    setMe(null)
    navigate('#/login')
  }

  // 加载中
  if (me === undefined || setupNeeded === undefined) {
    return <div className="flex min-h-svh items-center justify-center text-muted-foreground">加载中…</div>
  }

  // 未登录 / 首启向导
  if (!me) {
    return <Login setupNeeded={setupNeeded} onDone={() => { loadMe(); navigate('#/') }} />
  }

  return <Main
    hash={hash}
    navigate={navigate}
    me={me}
    view={view}
    changeView={changeView}
    group={group}
    setGroup={setGroup}
    onLogout={logout}
  />
}

function Main(props: {
  hash: string
  navigate: (h: string) => void
  me: Me
  view: ViewMode
  changeView: (v: ViewMode) => void
  group: number | 'all'
  setGroup: (g: number | 'all') => void
  onLogout: () => void
}) {
  const { hash, navigate, me, view, changeView, group, setGroup, onLogout } = props
  const { servers, status } = useServers()

  const list = useMemo(() => [...servers.values()], [servers])

  // 管理页
  if (hash === '#/admin') {
    if (me.role !== 'admin') {
      return <div className="py-20 text-center text-muted-foreground">需要管理员权限</div>
    }
    return (
      <Shell status={status} me={me} navigate={navigate} onLogout={onLogout} bare>
        <Admin />
      </Shell>
    )
  }

  // 详情页
  const detailId = hash.startsWith('#/server/') ? hash.slice('#/server/'.length) : null
  const detailServer = detailId ? servers.get(detailId) : null
  if (detailId) {
    return (
      <Shell status={status} me={me} navigate={navigate} onLogout={onLogout} bare>
        {detailServer ? (
          <ServerDetail server={detailServer} onBack={() => navigate('#/')} />
        ) : (
          <div className="py-20 text-center text-muted-foreground">加载中…</div>
        )}
      </Shell>
    )
  }

  // 仪表盘
  const groups = useMemo(() => {
    const map = new Map<number, number>()
    for (const s of list) if (s.group_id != null) map.set(s.group_id, (map.get(s.group_id) ?? 0) + 1)
    return [...map.entries()].map(([id, count]) => ({ id, name: `分组 ${id}`, count }))
  }, [list])

  const filtered = group === 'all' ? list : list.filter((s) => s.group_id === group)
  const sorted = [...filtered].sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name))

  return (
    <Shell status={status} me={me} navigate={navigate} onLogout={onLogout} view={view} changeView={changeView}>
      <div className="space-y-5">
        <StatCards servers={list} />
        {groups.length > 0 && <GroupTabs groups={groups} active={group} onChange={setGroup} />}
        {sorted.length === 0 ? (
          <div className="py-20 text-center text-muted-foreground">
            {list.length === 0 ? '还没有服务器，到「管理 → 服务器」添加一台' : '该分组暂无服务器'}
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
      </div>
    </Shell>
  )
}

function Shell(props: {
  status: string
  me: Me
  navigate: (h: string) => void
  onLogout: () => void
  bare?: boolean
  view?: ViewMode
  changeView?: (v: ViewMode) => void
  children: React.ReactNode
}) {
  const { status, me, navigate, onLogout, bare, view, changeView, children } = props
  return (
    <div className="min-h-svh bg-background text-foreground">
      <header className="sticky top-0 z-10 border-b bg-background/80 backdrop-blur">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-4 py-3">
          <button className="flex items-center gap-2" onClick={() => navigate('#/')}>
            <span className="text-lg font-semibold tracking-tight">zhengtang</span>
            <span
              className={cn(
                'size-2 rounded-full',
                status === 'open' ? 'bg-emerald-500' : status === 'connecting' ? 'bg-amber-500 animate-pulse' : 'bg-red-500',
              )}
              title={status === 'open' ? '实时连接正常' : status === 'connecting' ? '连接中…' : '已断开，自动重连中'}
            />
          </button>
          <div className="flex items-center gap-2">
            <span className="text-sm text-muted-foreground">{me.username}</span>
            {me.role === 'admin' && (
              <button
                className="inline-flex size-8 items-center justify-center rounded-md border text-muted-foreground hover:bg-accent hover:text-foreground"
                title="管理"
                onClick={() => navigate('#/admin')}
              >
                <Settings className="size-4" />
              </button>
            )}
            {view && changeView && <ViewModeToggle value={view} onChange={changeView} />}
            <ThemeToggle />
            <button
              className="inline-flex size-8 items-center justify-center rounded-md border text-muted-foreground hover:bg-accent hover:text-foreground"
              title="退出登录"
              onClick={onLogout}
            >
              <LogOut className="size-4" />
            </button>
          </div>
        </div>
      </header>
      <main className={`mx-auto max-w-6xl px-4 ${bare ? 'py-4' : 'py-6'}`}>{children}</main>
      {!bare && (
        <footer className="pb-8 text-center text-xs text-muted-foreground">
          zhengtang · 严格只读探针
        </footer>
      )}
    </div>
  )
}

