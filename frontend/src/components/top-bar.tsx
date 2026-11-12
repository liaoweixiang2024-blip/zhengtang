// 全站统一顶栏（jikasei 同款结构）：
// 站名（图标+粗体，hover 透明度）… 弹性空隙 … 搜索 | 图标条带（账号/卡片形态/主题）
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Button } from '@/components/ui/button'
import { CardStyleMenu } from '@/components/card-style-menu'
import { SearchBox } from '@/components/search-box'
import { ThemeToggle } from '@/components/theme-toggle'
import type { ViewMode } from '@/components/dashboard-header'
import { NAV } from '@/pages/admin'
import type { Me } from '@/lib/types'
import { cn } from '@/lib/utils'
import { Bell, LogOut, UserRound } from 'lucide-react'

export function TopBar(props: {
  siteName: string
  /** 实时连接状态（登录后才有） */
  status?: string
  /** undefined=加载中（不渲染用户区）；null=未登录 */
  me?: Me | null
  /** 登录页自身不需要"登录"按钮 */
  showLogin?: boolean
  navigate: (h: string, replace?: boolean) => void
  onLogout: () => void
  /** 传入则显示卡片形态菜单与搜索（列表页） */
  view?: ViewMode
  changeView?: (v: ViewMode) => void
  search?: { value: string; onChange: (v: string) => void }
}) {
  const { siteName, status, me, showLogin = true, navigate, onLogout, view, changeView, search } = props
  return (
    <header className="sticky top-0 z-30 border-b bg-background/80 backdrop-blur">
      <div className="mx-auto flex w-full max-w-[1280px] items-center gap-2 px-4 py-3 sm:gap-3 sm:px-6">
        {/* 站名：图标 + 粗体；点击回首页（jikasei：hover 透明度而非底色） */}
        <button
          className="flex max-w-[55%] shrink-0 items-center gap-2.5 font-semibold transition-opacity hover:opacity-70"
          onClick={() => navigate('/')}
        >
          <img src="/favicon.svg" alt="" className="size-8 shrink-0 rounded-full object-cover" />
          <span className="truncate text-base">{siteName}</span>
          {status && (
            <span
              className={cn(
                'size-2 shrink-0 rounded-full',
                status === 'open' ? 'bg-emerald-500' : status === 'connecting' ? 'bg-amber-500 animate-pulse' : 'bg-red-500',
              )}
              title={status === 'open' ? '实时连接正常' : status === 'connecting' ? '连接中…' : '已断开，自动重连中'}
            />
          )}
        </button>
        <div className="flex-1" />
        {search && <SearchBox value={search.value} onChange={search.onChange} />}
        {/* 图标条带：装不下就横向滑动（jikasei header-tools） */}
        <div className="-my-1.5 flex min-w-0 items-center gap-1.5 overflow-x-auto px-1.5 py-1.5 sm:gap-3">
          {me === undefined ? null : me ? (
            <>
              {view && changeView && <CardStyleMenu value={view} onPick={changeView} />}
              <UserMenu me={me} navigate={navigate} onLogout={onLogout} />
            </>
          ) : showLogin ? (
            <Button variant="ghost" size="sm" onClick={() => navigate('/login')}>登录</Button>
          ) : null}
          <ThemeToggle />
        </div>
      </div>
    </header>
  )
}

/** 账号菜单：ghost 图标按钮（与条带其他图标同规格）+ 悬停/点击展开后台菜单。
 *  浮层 portal 挂到 header（条带 overflow-x-auto 会裁掉直接展开的浮层）。 */
function UserMenu({
  me, navigate, onLogout,
}: {
  me: Me
  navigate: (h: string, replace?: boolean) => void
  onLogout: () => void
}) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const menu = useRef<HTMLDivElement>(null)
  const [spot, setSpot] = useState<{ host: HTMLElement; top: number; right: number } | null>(null)

  useLayoutEffect(() => {
    if (!open) return
    const place = () => {
      const b = root.current?.getBoundingClientRect()
      const host = root.current?.closest<HTMLElement>('header')
      const h = host?.getBoundingClientRect()
      if (b && host && h) setSpot({ host, top: b.bottom - h.top + 4, right: h.right - b.right })
    }
    place()
    window.addEventListener('resize', place)
    return () => window.removeEventListener('resize', place)
  }, [open])

  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      const el = e.target
      if (!(el instanceof HTMLElement) || !(root.current?.contains(el) || menu.current?.contains(el))) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('pointerdown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  const admin = me.role === 'admin'
  const item =
    'flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-sm transition-colors hover:bg-accent'

  return (
    <div
      ref={root}
      className="relative inline-flex"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      <Button
        variant="ghost"
        size="icon"
        title={`${me.username} · 账号菜单`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <UserRound />
      </Button>
      {open && spot && createPortal(
        <div
          ref={menu}
          role="menu"
          style={{ top: spot.top, right: spot.right }}
          className="absolute z-50 block w-44 overflow-hidden rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg"
        >
          {!admin && (
            <button type="button" role="menuitem" className={item} onClick={() => { setOpen(false); navigate('/alerts') }}>
              <Bell className="size-4" />
              告警中心
            </button>
          )}
          {admin &&
            NAV.map(({ key, label, icon: Icon }) => (
              <button
                key={key}
                type="button"
                role="menuitem"
                className={item}
                onClick={() => { setOpen(false); navigate(`/admin?tab=${key}`) }}
              >
                <Icon className="size-4" />
                {label}
              </button>
            ))}
          <div className="my-1 h-px bg-border" />
          <button
            type="button"
            role="menuitem"
            className={cn(item, 'text-destructive hover:text-destructive')}
            onClick={() => { setOpen(false); onLogout() }}
          >
            <LogOut className="size-4" />
            退出登录
          </button>
        </div>,
        spot.host,
      )}
    </div>
  )
}
