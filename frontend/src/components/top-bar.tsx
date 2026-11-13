// 全站统一顶栏（jikasei 同款结构）：
// 站名（图标+粗体，hover 透明度）… 弹性空隙 … 搜索 | 图标条带（账号/卡片形态/主题）
import { Button } from '@/components/ui/button'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel,
  DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
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
    <header className="sticky top-0 z-30 bg-background/80 backdrop-blur">
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

/** 账号菜单：shadcn DropdownMenu（点击展开——Radix 原生行为，悬停受控会与
 *  浮层定位形成开合循环导致闪烁） */
export function UserMenu({
  me, navigate, onLogout,
}: {
  me: Me
  navigate: (h: string, replace?: boolean) => void
  onLogout: () => void
}) {
  const admin = me.role === 'admin'
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" title={`${me.username} · 账号菜单`} aria-label="账号菜单">
          <UserRound />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuLabel>
          <div className="truncate text-sm font-medium">{me.username}</div>
          <div className="text-xs font-normal text-muted-foreground">{admin ? '管理员' : '观察者'}</div>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        {!admin && (
          <DropdownMenuItem onClick={() => navigate('/alerts')}>
            <Bell />
            告警中心
          </DropdownMenuItem>
        )}
        {admin &&
          NAV.map(({ key, label, icon: Icon }) => (
            <DropdownMenuItem key={key} onClick={() => navigate(`/admin?tab=${key}`)}>
              <Icon />
              {label}
            </DropdownMenuItem>
          ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem variant="destructive" onClick={onLogout}>
          <LogOut />
          退出登录
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
