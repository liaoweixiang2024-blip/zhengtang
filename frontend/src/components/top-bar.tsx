// 全站统一顶栏：
// - 未登录：界面调节（明暗）+ 登录按钮
// - 已登录：界面调节 + 头像图标（悬停弹出下拉：后台菜单 + 退出登录）
import { useEffect, useRef, useState } from 'react'
import { ThemeToggle } from '@/components/theme-toggle'
import { ViewModeToggle, type ViewMode } from '@/components/dashboard-header'
import { NAV } from '@/pages/admin'
import type { Me } from '@/lib/types'
import { cn } from '@/lib/utils'
import { Bell, LogOut, UserRound } from 'lucide-react'

export function TopBar(props: {
  siteName: string
  /** 实时连接状态（登录后仪表盘才有） */
  status?: string
  /** undefined=加载中（不渲染用户区）；null=未登录 */
  me?: Me | null
  /** 登录页自身不需要"登录"按钮 */
  showLogin?: boolean
  navigate: (h: string, replace?: boolean) => void
  onLogout: () => void
  view?: ViewMode
  changeView?: (v: ViewMode) => void
}) {
  const { siteName, status, me, showLogin = true, navigate, onLogout, view, changeView } = props
  return (
    <header className="sticky top-0 z-30 border-b bg-background/80 backdrop-blur">
      <div className="mx-auto flex w-full max-w-[1280px] items-center justify-between gap-2 px-4 py-3 sm:px-6">
        {status ? (
          <button className="flex items-center gap-2" onClick={() => navigate('/')}>
            <span className="text-lg font-semibold tracking-tight">{siteName}</span>
            <span
              className={cn(
                'size-2 rounded-full',
                status === 'open' ? 'bg-emerald-500' : status === 'connecting' ? 'bg-amber-500 animate-pulse' : 'bg-red-500',
              )}
              title={status === 'open' ? '实时连接正常' : status === 'connecting' ? '连接中…' : '已断开，自动重连中'}
            />
          </button>
        ) : (
          <span className="text-lg font-semibold tracking-tight">{siteName}</span>
        )}
        <div className="flex items-center gap-1.5">
          {view && changeView && <ViewModeToggle value={view} onChange={changeView} />}
          <ThemeToggle />
          {me === undefined ? null : me ? (
            <UserMenu me={me} navigate={navigate} onLogout={onLogout} />
          ) : showLogin ? (
            <button
              onClick={() => navigate('/login')}
              className="rounded-full px-3.5 py-1.5 text-sm text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:bg-accent"
            >
              登录
            </button>
          ) : null}
        </div>
      </div>
    </header>
  )
}

/** 头像菜单：自绘下拉（悬停展开、无 Radix 受控状态 → 不闪烁）。
 *  触发器与菜单同容器：mouseenter/leave 天然覆盖两者，移入菜单不会误关。 */
function UserMenu({
  me, navigate, onLogout,
}: {
  me: Me
  navigate: (h: string, replace?: boolean) => void
  onLogout: () => void
}) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)

  // 点击外部 / Esc 关闭（悬停之外的补充：触屏、键盘）
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  const admin = me.role === 'admin'
  const item =
    'flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm transition-colors hover:bg-accent hover:text-foreground'

  return (
    <div ref={root} className="relative" onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}>
      <button
        type="button"
        title={`${me.username} · 账号菜单`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'inline-flex size-8 items-center justify-center rounded-full outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:bg-accent',
          open ? 'bg-accent text-foreground' : 'text-muted-foreground',
        )}
      >
        <UserRound className="size-[18px]" />
      </button>
      {open && (
        <div role="menu" className="absolute right-0 top-full z-50 min-w-44 pt-1.5">
          <div className="rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg">
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
          </div>
        </div>
      )}
    </div>
  )
}
