// 全站统一顶栏（jikasei 同款结构）：
// 站名（图标+粗体，hover 透明度）… 弹性空隙 … 搜索 | 图标条带（账号/卡片形态/主题）
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel,
  DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { CardStyleMenu } from '@/components/card-style-menu'
import { SearchBox } from '@/components/search-box'
import { ThemeToggle } from '@/components/theme-toggle'
import type { ViewMode } from '@/components/dashboard-header'
import { NAV } from '@/pages/admin'
import type { Me } from '@/lib/types'
import { cn } from '@/lib/utils'
import { Bell, KeyRound, LogOut, UserRound } from 'lucide-react'

export function TopBar(props: {
  siteName: string
  /** 自定义 LOGO（/uploads/…，空串回退默认 favicon.svg） */
  logo?: string
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
  const { siteName, logo, status, me, showLogin = true, navigate, onLogout, view, changeView, search } = props
  return (
    <header className="sticky top-0 z-30 bg-background/80 backdrop-blur">
      <div className="mx-auto flex w-full max-w-[1280px] items-center gap-2 px-4 py-3 sm:gap-3 sm:px-6">
        {/* 站名：图标 + 粗体；点击回首页（jikasei：hover 透明度而非底色） */}
        <button
          className="flex max-w-[55%] shrink-0 items-center gap-2.5 font-semibold transition-opacity hover:opacity-70"
          onClick={() => navigate('/')}
        >
          <img src={logo || '/favicon.svg'} alt="" className="size-8 shrink-0 rounded-full object-cover" />
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
  const [pwOpen, setPwOpen] = useState(false)
  return (
    <>
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
          <DropdownMenuItem onClick={() => setPwOpen(true)}>
            <KeyRound />
            修改密码
          </DropdownMenuItem>
          {!admin && (
            <DropdownMenuItem onClick={() => navigate('/alerts')}>
              <Bell />
              告警中心
            </DropdownMenuItem>
          )}
          {admin &&
            NAV.filter((n) => n.key !== 'update').map(({ key, label, icon: Icon }) => (
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
      <ChangePasswordDialog open={pwOpen} onOpenChange={setPwOpen} />
    </>
  )
}

/** 修改自己的密码（admin/viewer 通用；改完只保留当前会话，其他设备被踢出） */
function ChangePasswordDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const [oldPw, setOldPw] = useState('')
  const [newPw, setNewPw] = useState('')
  const [confirmPw, setConfirmPw] = useState('')
  const [error, setError] = useState('')
  const [ok, setOk] = useState(false)
  const [busy, setBusy] = useState(false)

  const submit = async () => {
    setError('')
    if (newPw.length < 8) {
      setError('新密码至少 8 位')
      return
    }
    if (newPw !== confirmPw) {
      setError('两次输入的新密码不一致')
      return
    }
    setBusy(true)
    const res = await fetch('/api/auth/password', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ old_password: oldPw, new_password: newPw }),
    })
    setBusy(false)
    if (!res.ok) {
      const d = await res.json().catch(() => ({}))
      setError(d.error ?? '修改失败')
      return
    }
    setOk(true)
    setTimeout(() => {
      setOk(false)
      setOldPw('')
      setNewPw('')
      setConfirmPw('')
      onOpenChange(false)
    }, 1200)
  }

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!busy) onOpenChange(v) }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>修改密码</DialogTitle>
          <DialogDescription>修改后其他设备的登录会话将被注销，当前设备不受影响。</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label>原密码</Label>
            <Input type="password" value={oldPw} onChange={(e) => setOldPw(e.target.value)} autoFocus />
          </div>
          <div className="space-y-1.5">
            <Label>新密码</Label>
            <Input type="password" value={newPw} onChange={(e) => setNewPw(e.target.value)} placeholder="≥8 位" />
          </div>
          <div className="space-y-1.5">
            <Label>确认新密码</Label>
            <Input
              type="password"
              value={confirmPw}
              onChange={(e) => setConfirmPw(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submit()}
            />
          </div>
          <p className="min-h-5 text-sm text-destructive">{error}</p>
          {ok && <p className="text-sm text-emerald-600">✓ 已修改</p>}
        </div>
        <DialogFooter>
          <Button onClick={submit} disabled={busy || !oldPw || !newPw}>
            {busy ? '提交中…' : '修改密码'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
