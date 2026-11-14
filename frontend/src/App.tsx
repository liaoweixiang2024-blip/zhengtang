// 路由：/ 仪表盘 | /server/{id} 详情 | /login 登录/向导 | /admin 管理 | /alerts 告警 | /public 公开页
import { useCallback, useEffect, useMemo, useState } from 'react'
import { GroupTabs, StatCards, type ViewMode } from '@/components/dashboard-header'
import { Globe } from '@/components/globe-view'
import { regionCode } from '@/components/server-card'
import { regionOf, regionView } from '@/lib/globe'
import { TopBar } from '@/components/top-bar'
import { ServerCard, ServerCardMini } from '@/components/server-card'
import { ServerTable } from '@/components/server-table'
import { Login } from '@/pages/login'
import { Admin, parseAdminTab } from '@/pages/admin'
import { Alerts } from '@/pages/alerts'
import { PublicPage } from '@/pages/public'
import { ServerDetail } from '@/pages/server-detail'
import { useServers } from '@/hooks/useServers'
import type { Me } from '@/lib/types'

const VIEW_KEY = 'zt-view-mode'

/// History 路由：pathname + pushState（URL 干净：/admin、/server/xxx）。
/// 服务端对非 /api /ws 路径一律 fallback 到 SPA，刷新/直开/分享均可。
function usePathRoute(): [string, (to: string, replace?: boolean) => void] {
  const [path, setPath] = useState(() => location.pathname + location.search || '/')
  useEffect(() => {
    const on = () => setPath(location.pathname + location.search || '/')
    addEventListener('popstate', on)
    return () => removeEventListener('popstate', on)
  }, [])
  const navigate = (to: string, replace = false) => {
    if (replace) history.replaceState(null, '', to)
    else history.pushState(null, '', to)
    // 切页回顶：旧页的滚动位置若带进新页，内容高度一变视口就"跳一下"
    window.scrollTo(0, 0)
    setPath(to)
  }
  return [path, navigate]
}

interface Site { site_name: string; site_description: string; public_page_enabled: boolean; globe_enabled: boolean; site_logo: string; footer_text: string }

export default function App() {
  const [path, navigate] = usePathRoute()
  const [me, setMe] = useState<Me | null | undefined>(undefined) // undefined=加载中
  const [setupNeeded, setSetupNeeded] = useState(false)
  const [site, setSite] = useState<Site>({ site_name: 'zhengtang', site_description: '', public_page_enabled: true, globe_enabled: true, site_logo: '', footer_text: '' })
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

  const loadStatus = useCallback(() => {
    fetch('/api/status')
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => {
        if (!d) return
        setSetupNeeded(!!d.setup_needed)
        const s: Site = {
          site_name: d.site_name || 'zhengtang',
          site_description: d.site_description || '',
          public_page_enabled: d.public_page_enabled !== false,
          globe_enabled: d.globe_enabled !== false,
          site_logo: d.site_logo || '',
          footer_text: d.footer_text || '',
        }
        setSite(s)
        // SEO：动态 title/description（SPA 环境下对执行 JS 的爬虫生效）
        document.title = `${s.site_name} · 服务器探针`
        if (s.site_description) {
          document.querySelector('meta[name=description]')?.setAttribute('content', s.site_description)
        }
        // 自定义 LOGO：浏览器标签页图标一并替换
        document.querySelector('link[rel="icon"]')?.setAttribute('href', s.site_logo || '/favicon.svg')
      })
      .catch(() => setSetupNeeded(false))
  }, [])
  useEffect(loadStatus, [loadStatus])
  // 后台改了 LOGO/站名：广播一下，前台立即跟上（免整页刷新）
  useEffect(() => {
    const on = () => loadStatus()
    addEventListener('zt-site-changed', on)
    return () => removeEventListener('zt-site-changed', on)
  }, [loadStatus])

  const changeView = (v: ViewMode) => {
    setView(v)
    localStorage.setItem(VIEW_KEY, v)
  }

  const logout = async () => {
    await fetch('/api/auth/logout', { method: 'POST' })
    setMe(null)
    navigate('/login')
  }

  // 公开展示页：匿名可访问（在登录判断之前）；总开关关闭时提示
  if (path === '/public') {
    return site.public_page_enabled ? (
      <div className="min-h-svh bg-background text-foreground">
        <TopBar siteName={site.site_name} logo={site.site_logo} me={me ?? null} navigate={navigate} onLogout={logout} />
        <PublicPage />
      </div>
    ) : (
      <div className="flex min-h-svh items-center justify-center text-muted-foreground">公开页未开放</div>
    )
  }

  // 加载中
  if (me === undefined) {
    return <div className="flex min-h-svh items-center justify-center text-muted-foreground">加载中…</div>
  }

  // 未登录 / 首启向导：同样带顶栏（界面调节 + 登录按钮）
  if (!me) {
    return (
      <div className="flex min-h-svh flex-col bg-background text-foreground">
        <TopBar
          siteName={site.site_name}
          logo={site.site_logo}
          me={null}
          showLogin={path !== '/login'}
          navigate={navigate}
          onLogout={logout}
        />
        <div className="flex flex-1 items-center justify-center p-4">
          <Login
            setupNeeded={setupNeeded}
            siteName={site.site_name}
            onDone={() => {
              loadMe()
              navigate(path && path !== '/login' ? path : '/')
            }}
          />
        </div>
      </div>
    )
  }

  return <Main
    siteName={site.site_name}
    siteLogo={site.site_logo}
    path={path}
    navigate={navigate}
    me={me}
    view={view}
    changeView={changeView}
    group={group}
    setGroup={setGroup}
    onLogout={logout}
    globeEnabled={site.globe_enabled}
    footerText={site.footer_text}
  />
}

function Main(props: {
  siteName: string
  siteLogo: string
  path: string
  navigate: (h: string, replace?: boolean) => void
  me: Me
  view: ViewMode
  changeView: (v: ViewMode) => void
  group: number | 'all'
  setGroup: (g: number | 'all') => void
  onLogout: () => void
  globeEnabled: boolean
  footerText: string
}) {
  const { siteName, siteLogo, path, navigate, me, view, changeView, group, setGroup, onLogout, globeEnabled, footerText } = props
  const { servers, status, ready } = useServers()
  // 暗色（地球扫掠线透明度用）：跟着 <html class=dark> 走
  const [dark, setDark] = useState(() => document.documentElement.classList.contains('dark'))
  useEffect(() => {
    const mo = new MutationObserver(() => setDark(document.documentElement.classList.contains('dark')))
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
    return () => mo.disconnect()
  }, [])
  // 地区筛选（地球侧栏，叠加在分组之上）
  const [region, setRegion] = useState<string | null>(null)
  // 分组 id → 名称（真实组名来自 /api/groups；拉取失败回退"分组 N"）
  const [groupNames, setGroupNames] = useState<Map<number, string>>(new Map())
  useEffect(() => {
    fetch('/api/groups')
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => {
        if (!d?.groups) return
        setGroupNames(new Map(d.groups.map((g: { id: number; name: string }) => [g.id, g.name])))
      })
      .catch(() => {})
  }, [])

  const list = useMemo(() => [...servers.values()], [servers])
  // ⚠ 全部 hooks 必须在任何条件 return 之前（否则路由切换时 hook 数变化 → React 崩溃）
  const groups = useMemo(() => {
    const map = new Map<number, number>()
    for (const s of list) if (s.group_id != null) map.set(s.group_id, (map.get(s.group_id) ?? 0) + 1)
    return [...map.entries()].map(([id, count]) => ({ id, name: groupNames.get(id) ?? `分组 ${id}`, count }))
  }, [list, groupNames])

  // 告警中心（独立路由，观察者从这里进；管理员的入口在后台侧边栏）
  if (path === '/alerts') {
    return (
      <Shell siteName={siteName} siteLogo={siteLogo} footerText={footerText} status={status} me={me} navigate={navigate} onLogout={onLogout} bare>
        <Alerts isAdmin={me.role === 'admin'} />
      </Shell>
    )
  }

  // 管理页（?tab= 深链由顶栏用户菜单使用）
  if (path === '/admin' || path.startsWith('/admin')) {
    if (me.role !== 'admin') {
      return (
        <Shell siteName={siteName} siteLogo={siteLogo} footerText={footerText} status={status} me={me} navigate={navigate} onLogout={onLogout} bare>
          <div className="py-20 text-center text-muted-foreground">需要管理员权限</div>
        </Shell>
      )
    }
    return (
      <Shell siteName={siteName} siteLogo={siteLogo} footerText={footerText} status={status} me={me} navigate={navigate} onLogout={onLogout} bare>
        <Admin
          tab={parseAdminTab(path.split('?')[1] ?? '')}
          onTabChange={(k) => navigate(`/admin?tab=${k}`, true)}
          navigate={navigate}
        />
      </Shell>
    )
  }

  // 详情页
  const detailId = path.startsWith('/server/') ? decodeURIComponent(path.slice('/server/'.length)) : null
  const detailServer = detailId ? servers.get(detailId) : null
  if (detailId) {
    return (
      <Shell siteName={siteName} siteLogo={siteLogo} footerText={footerText} status={status} me={me} navigate={navigate} onLogout={onLogout} bare>
        {detailServer ? (
          <ServerDetail server={detailServer} onBack={() => navigate('/')} />
        ) : status === 'open' ? (
          // WS 已连通快照仍无此机器：不存在或无权限（不再永久"加载中"卡死）
          <div className="flex flex-col items-center gap-3 py-20 text-muted-foreground">
            <p>服务器不存在或无权访问</p>
            <button className="text-sm underline underline-offset-4" onClick={() => navigate('/')}>
              返回仪表盘
            </button>
          </div>
        ) : (
          <div className="py-20 text-center text-muted-foreground">
            连接实时数据中…
            <div className="mt-2">
              <button className="text-sm underline underline-offset-4" onClick={() => navigate('/')}>
                返回仪表盘
              </button>
            </div>
          </div>
        )}
      </Shell>
    )
  }

  // 仪表盘
  const filtered = group === 'all' ? list : list.filter((s) => s.group_id === group)
  // 地球输入（分组后的机器；country 供落针与地区列表）
  const gnodes = filtered.map((s) => ({ id: s.id, name: s.name, online: s.online, country: regionCode(s.region) }))
  const gview = regionView(gnodes, region)
  // 筛选按国家码匹配（城市机的 key 是城市级，region 侧栏现在是国家级）
  const finalList = region == null ? filtered : filtered.filter((_, i) => { const r = regionOf(gnodes[i]); return r ? r.key === region || (r.code || r.key) === region : false })
  // 顺序 = 面板手动序（后台拖拽调序实时同步到这里；不再按在线/名字本地重排）
  const sorted = finalList
  const hasCountry = gnodes.some((n) => n.country)

  return (
    <Shell
      siteName={siteName}
      siteLogo={siteLogo}
      footerText={footerText}
      status={status}
      me={me}
      navigate={navigate}
      onLogout={onLogout}
      view={view}
      changeView={changeView}
    >
      <div className="space-y-5">
        <StatCards servers={list} />
        {/* 节点地球（后台可开关）：左地球（针=机器，可拖转/点开）右地区列表（点击筛选） */}
        {globeEnabled && hasCountry && (
          <Globe
            nodes={gnodes}
            dark={dark}
            region={gview.current}
            onRegion={setRegion}
            onOpen={(id) => navigate(`/server/${id}`)}
            onWarm={() => {}}
          />
        )}
        {groups.length > 0 && <GroupTabs groups={groups} active={group} onChange={setGroup} />}
        {sorted.length === 0 ? (
          list.length === 0 ? (
            ready ? (
              // 首帧已到且确为空：才是真"还没有服务器"（HTTP 预取与 WS 快照都到了）
              <div className="py-20 text-center text-muted-foreground">
                还没有服务器，到「后台 → 主机」添加一台
              </div>
            ) : (
              // 数据在途：卡片形状的脉冲骨架——刷新时不再闪"还没有服务器"的字样
              <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
                {[0, 1, 2].map((i) => (
                  <div key={i} className="h-44 animate-pulse rounded-xl border border-border/60 bg-muted/30" />
                ))}
              </div>
            )
          ) : (
            <div className="py-20 text-center text-muted-foreground">该分组暂无服务器</div>
          )
        ) : view === 'compact' ? (
          <ServerTable servers={sorted} onOpen={(id) => navigate(`/server/${id}`)} />
        ) : view === 'mini' ? (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {sorted.map((s) => (
              <button key={s.id} className="text-left" onClick={() => navigate(`/server/${s.id}`)}>
                <ServerCardMini server={s} />
              </button>
            ))}
          </div>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
            {sorted.map((s) => (
              <button key={s.id} className="text-left" onClick={() => navigate(`/server/${s.id}`)}>
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
  siteName: string
  siteLogo: string
  status: string
  me: Me
  navigate: (h: string, replace?: boolean) => void
  onLogout: () => void
  bare?: boolean
  /** 全宽容器（后台 Sidebar 套件需要：其侧栏为视口 fixed 定位） */
  full?: boolean
  view?: ViewMode
  changeView?: (v: ViewMode) => void
  footerText: string
  children: React.ReactNode
}) {
  const { siteName, siteLogo, status, me, navigate, onLogout, bare, full, view, changeView, footerText, children } = props
  return (
    <div className="flex min-h-svh flex-col bg-background text-foreground">
      <TopBar
        siteName={siteName}
        logo={siteLogo}
        status={status}
        me={me}
        navigate={navigate}
        onLogout={onLogout}
        view={view}
        changeView={changeView}
      />
      <main
        className={
          full
            ? 'flex-1'
            : `mx-auto w-full max-w-[1280px] flex-1 px-4 sm:px-6 ${bare ? 'py-4' : 'py-6'}`
        }
      >
        {children}
      </main>
      {!bare && (
        <footer className="mt-auto pb-6 pt-2 text-center text-xs text-muted-foreground">
          {footerText || 'zhengtang · 严格只读探针'}
        </footer>
      )}
    </div>
  )
}
