// 管理页：服务器管理 / 分组 / 用户 / 审计日志 / 告警中心 / 设置
import { useCallback, useEffect, useRef, useState } from 'react'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { Alerts } from '@/pages/alerts'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { fmtBytes } from '@/lib/format'
import { CYCLE_LABEL, CURRENCY_SYMBOL, fmtCost, monthlyCostByCurrency } from '@/lib/cost'
import type { ServerView } from '@/lib/types'
import { Bell, Info, ListTree, Pencil, Plus, Radar, ScrollText, Server, Settings, Trash2, Users, FolderGit2, KeyRound } from 'lucide-react'

interface Group { id: number; name: string }
interface User { id: number; username: string; role: string; totp_enabled: boolean }
interface AuditLog { id: number; user: string | null; action: string; detail: string; ip: string | null; at: number }

// 侧边导航（顶栏用户菜单也复用这份清单）
export const NAV = [
  { key: 'servers', label: '主机', icon: Server },
  { key: 'groups', label: '分组', icon: FolderGit2 },
  { key: 'users', label: '用户', icon: Users },
  { key: 'audit', label: '审计', icon: ScrollText },
  { key: 'alerts', label: '告警', icon: Bell },
  { key: 'settings', label: '设置', icon: Settings },
  { key: 'update', label: '关于', icon: Info },
] as const

export type NavKey = (typeof NAV)[number]['key']

/** 从 /admin?tab=xxx 解析合法 tab */
export function parseAdminTab(query: string): NavKey {
  const t = new URLSearchParams(query).get('tab')
  return NAV.some((n) => n.key === t) ? (t as NavKey) : 'servers'
}

export function Admin({ tab, onTabChange, navigate }: {
  tab?: NavKey
  onTabChange?: (k: NavKey) => void
  navigate?: (h: string) => void
}) {
  const [inner, setInner] = useState<NavKey>('servers')
  const [version, setVersion] = useState('')
  const active = tab ?? inner
  const select = (k: NavKey) => {
    setInner(k)
    onTabChange?.(k)
  }
  useEffect(() => {
    fetch('/api/status').then((r) => r.json()).then((d) => setVersion(d.version ?? '')).catch(() => {})
  }, [])
  return (
    <div className="flex flex-col gap-6 md:flex-row md:flex-wrap">
      {/* 侧边导航（monitor-probe 同款）：窄屏横向滚动，宽屏左列 44 宽；激活=浅底半粗 */}
      <nav className="flex gap-1 overflow-x-auto pb-1 md:w-44 md:shrink-0 md:flex-col md:gap-0.5 md:overflow-visible md:pb-0">
        {NAV.map(({ key, label, icon: Icon }) => {
          const isActive = active === key
          return (
            <button
              key={key}
              onClick={() => select(key)}
              aria-current={isActive ? 'page' : undefined}
              className={`flex shrink-0 items-center gap-2 rounded-md px-3 py-2 text-sm font-medium outline-none transition-colors ${
                isActive
                  ? 'bg-secondary text-secondary-foreground'
                  : 'text-muted-foreground hover:bg-muted'
              }`}
            >
              <Icon className="size-4" />
              {label}
            </button>
          )
        })}
      </nav>
      <div className="min-w-0 flex-1">
        {active === 'servers' && <ServersTab navigate={navigate} />}
        {active === 'groups' && <GroupsTab />}
        {active === 'users' && <UsersTab />}
        {active === 'audit' && <AuditTab />}
        {active === 'alerts' && <Alerts isAdmin />}
        {active === 'settings' && <SettingsTab />}
        {active === 'update' && <UpdateTab />}
      </div>
      {/* 页脚：版权 + 版本号 + 站点设置的快捷入口 */}
      <footer className="flex w-full flex-wrap items-center justify-center gap-x-3 gap-y-1 border-t border-border pt-4 text-xs text-muted-foreground">
        <span>© {new Date().getFullYear()} zhengtang · 严格只读探针</span>
        {version && <span className="tabular-nums">v{version}</span>}
        <button
          type="button"
          className="underline-offset-2 transition-colors hover:text-foreground hover:underline"
          onClick={() => select('settings')}
        >
          站点设置
        </button>
      </footer>
    </div>
  )
}

// ---------- 服务器管理 ----------

function ServersTab({ navigate }: { navigate?: (h: string) => void }) {
  const [servers, setServers] = useState<ServerView[]>([])
  const [groups, setGroups] = useState<Group[]>([])
  const [addOpen, setAddOpen] = useState(false)
  const [installCmd, setInstallCmd] = useState<string | null>(null)
  const [dialServer, setDialServer] = useState<ServerView | null>(null)
  const [procServer, setProcServer] = useState<ServerView | null>(null)
  const [editServer, setEditServer] = useState<ServerView | null>(null)
  const [delServer, setDelServer] = useState<ServerView | null>(null)
  const openDetail = (id: string) => (navigate ? navigate(`/server/${id}`) : location.assign(`/server/${id}`))

  const load = useCallback(() => {
    fetch('/api/admin/servers').then((r) => r.json()).then((d) => setServers(d.servers ?? []))
    fetch('/api/admin/groups').then((r) => r.json()).then((d) => setGroups(d.groups ?? []))
  }, [])
  useEffect(load, [load])

  const add = async (name: string) => {
    const origin = location.origin.replace(/^http/, 'ws').replace(':5173', ':8800')
    const d = await fetch('/api/admin/servers', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name, server_url: origin }),
    }).then((r) => r.json())
    setInstallCmd(d.install_cmd_short ?? d.install_cmd ?? '生成失败')
    load()
  }

  const remove = async (id: string) => {
    await fetch(`/api/admin/servers/${id}`, { method: 'DELETE' })
    setDelServer(null)
    load()
  }

  // 各货币月均费用合计
  const costStats = monthlyCostByCurrency(servers)

  return (
    <Card className="py-4">
      <CardHeader className="flex-row items-center justify-between">
        <div>
          <CardTitle className="text-base">服务器（{servers.length}）</CardTitle>
          {costStats.length > 0 && (
            <p className="mt-1 text-xs text-muted-foreground">
              月均合计：{costStats.map((c) => `${CURRENCY_SYMBOL[c.currency] ?? c.currency}${c.monthly.toFixed(1)}/月`).join(' · ')}
            </p>
          )}
        </div>
        <AddServerDialog open={addOpen} onOpenChange={setAddOpen} onAdd={add} installCmd={installCmd} onCopy={() => setInstallCmd(null)} />
        {dialServer && <DialDialog server={dialServer} onClose={() => setDialServer(null)} />}
        {procServer && <ProcDialog server={procServer} onClose={() => setProcServer(null)} />}
        {editServer && <EditServerDialog server={editServer} groups={groups} onClose={() => setEditServer(null)} onSaved={load} />}
        {delServer && (
          <ConfirmDialog
            open
            onOpenChange={(v) => !v && setDelServer(null)}
            title={`删除服务器 ${delServer.name}？`}
            description="其历史指标、流量与拨测数据将一并删除，不可恢复。"
            onConfirm={() => remove(delServer.id)}
          />
        )}
      </CardHeader>
      <CardContent>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>名称</TableHead>
              <TableHead className="hidden md:table-cell">到期</TableHead>
              <TableHead className="hidden sm:table-cell">流量配额</TableHead>
              <TableHead className="hidden lg:table-cell">费用</TableHead>
              <TableHead className="hidden xl:table-cell">购买/重置</TableHead>
              <TableHead className="w-10">编辑</TableHead>
              <TableHead className="w-10">拨测</TableHead>
              <TableHead className="w-10">进程</TableHead>
              <TableHead className="w-10"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {servers.map((s) => (
              <TableRow key={s.id} className="cursor-pointer" onClick={() => openDetail(s.id)} title="点击查看详情">
                <TableCell className="font-medium">{s.name}</TableCell>
                <TableCell className="hidden text-xs md:table-cell">{fmtExpiresCell(s.expires_at)}</TableCell>
                <TableCell className="hidden text-xs text-muted-foreground sm:table-cell">
                  {s.traffic_quota_bytes ? fmtBytes(s.traffic_quota_bytes) : '不限'}
                </TableCell>
                <TableCell className="hidden text-xs text-muted-foreground lg:table-cell">{fmtCost(s)}</TableCell>
                <TableCell className="hidden text-xs text-muted-foreground xl:table-cell">
                  {s.purchased_at ? dateStr(s.purchased_at) : '—'} · {s.traffic_reset_day === 99 ? '每月月末' : s.traffic_reset_day ? `每月${s.traffic_reset_day}日` : '自然月'}
                </TableCell>
                <TableCell onClick={(e) => e.stopPropagation()}>
                  <Button variant="ghost" size="icon" title="编辑" onClick={() => setEditServer(s)}>
                    <Pencil className="size-4 text-muted-foreground" />
                  </Button>
                </TableCell>
                <TableCell onClick={(e) => e.stopPropagation()}>
                  <Button variant="ghost" size="icon" title="拨测任务" onClick={() => setDialServer(s)}>
                    <Radar className="size-4 text-muted-foreground" />
                  </Button>
                </TableCell>
                <TableCell onClick={(e) => e.stopPropagation()}>
                  <Button variant="ghost" size="icon" title="进程监控" onClick={() => setProcServer(s)}>
                    <ListTree className="size-4 text-muted-foreground" />
                  </Button>
                </TableCell>
                <TableCell onClick={(e) => e.stopPropagation()}>
                  <Button variant="ghost" size="icon" title="删除" onClick={() => setDelServer(s)}>
                    <Trash2 className="size-4 text-muted-foreground" />
                  </Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  )
}

function dateStr(ts: number): string {
  return new Date(ts * 1000).toISOString().slice(0, 10)
}

// ---------- 费用展示（实现在 lib/cost.ts） ----------

function fmtExpiresCell(ts: number | null): string {
  if (!ts) return '长期'
  const days = Math.floor((ts * 1000 - Date.now()) / 86400000)
  const d = dateStr(ts)
  return days < 0 ? `${d}（已过期）` : `${d}（${days}天）`
}

function AddServerDialog({
  open, onOpenChange, onAdd, installCmd, onCopy,
}: {
  open: boolean
  onOpenChange: (v: boolean) => void
  onAdd: (name: string) => void
  installCmd: string | null
  onCopy: () => void
}) {
  const [name, setName] = useState('')
  return (
    <Dialog open={open} onOpenChange={(v) => { onOpenChange(v); if (!v) onCopy() }}>
      <DialogTrigger asChild>
        <Button size="sm"><Plus className="size-4" /> 添加</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>添加服务器</DialogTitle>
          <DialogDescription>
            {installCmd
              ? '在目标服务器上执行（安装令牌 24 小时内有效，删除该服务器即吊销）：'
              : '输入名称生成安装命令'}
          </DialogDescription>
        </DialogHeader>
        {installCmd ? (
          <div className="space-y-2">
            <pre className="max-h-40 overflow-auto rounded-lg bg-muted/50 p-3 text-xs break-all whitespace-pre-wrap">{installCmd}</pre>
            <p className="text-xs text-muted-foreground">
              装好后日常管理：<code className="rounded bg-muted px-1">zt-agent status | log | restart | upgrade | uninstall</code>
            </p>
            <Button
              size="sm"
              onClick={() => {
                navigator.clipboard.writeText(installCmd)
                onOpenChange(false)
                onCopy()
              }}
            >
              复制并关闭
            </Button>
          </div>
        ) : (
          <form
            className="space-y-3"
            onSubmit={(e) => {
              e.preventDefault()
              if (name.trim()) onAdd(name.trim())
              setName('')
            }}
          >
            <div className="space-y-2">
              <Label>名称</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="如 hk-web-01" autoFocus />
            </div>
            <Button type="submit" className="w-full">生成安装命令</Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}

// ---------- 分组 ----------

function GroupsTab() {
  const [groups, setGroups] = useState<Group[]>([])
  const [name, setName] = useState('')
  const [delGroup, setDelGroup] = useState<Group | null>(null)

  const load = useCallback(() => {
    fetch('/api/admin/groups').then((r) => r.json()).then((d) => setGroups(d.groups ?? []))
  }, [])
  useEffect(load, [load])

  const add = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    await fetch('/api/admin/groups', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name.trim() }),
    })
    setName('')
    load()
  }

  return (
    <Card className="py-4">
      <CardHeader><CardTitle className="text-base">分组</CardTitle></CardHeader>
      <CardContent className="space-y-3">
        <form onSubmit={add} className="flex gap-2">
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="新分组名称" className="max-w-60" />
          <Button type="submit" size="sm"><Plus className="size-4" /> 添加</Button>
        </form>
        <Table>
          <TableBody>
            {groups.map((g) => (
              <TableRow key={g.id}>
                <TableCell>{g.name}</TableCell>
                <TableCell className="w-10">
                  <Button variant="ghost" size="icon" title="删除" onClick={() => setDelGroup(g)}>
                    <Trash2 className="size-4 text-muted-foreground" />
                  </Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        {delGroup && (
          <ConfirmDialog
            open
            onOpenChange={(v) => !v && setDelGroup(null)}
            title={`删除分组 ${delGroup.name}？`}
            description="服务器将变为未分组，不会删除服务器本身。"
            onConfirm={async () => {
              await fetch(`/api/admin/groups/${delGroup.id}`, { method: 'DELETE' })
              setDelGroup(null)
              load()
            }}
          />
        )}
      </CardContent>
    </Card>
  )
}

// ---------- 用户 ----------

function UsersTab() {
  const [users, setUsers] = useState<User[]>([])
  const [groups, setGroups] = useState<Group[]>([])
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [role, setRole] = useState('viewer')
  const [groupIds, setGroupIds] = useState<number[]>([])
  const [error, setError] = useState('')
  const [delUser, setDelUser] = useState<User | null>(null)
  const [resetUser, setResetUser] = useState<User | null>(null)
  const [newPass, setNewPass] = useState('')
  const [resetMsg, setResetMsg] = useState('')

  const load = useCallback(() => {
    fetch('/api/admin/users').then((r) => r.json()).then((d) => setUsers(d.users ?? []))
    fetch('/api/admin/groups').then((r) => r.json()).then((d) => setGroups(d.groups ?? []))
  }, [])
  useEffect(load, [load])

  const add = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    const res = await fetch('/api/admin/users', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, password, role, group_ids: groupIds }),
    })
    const d = await res.json()
    if (!res.ok) {
      setError(d.error ?? '创建失败')
      return
    }
    setUsername('')
    setPassword('')
    setGroupIds([])
    load()
  }

  return (
    <Card className="py-4">
      <CardHeader><CardTitle className="text-base">用户</CardTitle></CardHeader>
      <CardContent className="space-y-4">
        <form onSubmit={add} className="grid gap-2 sm:grid-cols-5">
          <Input value={username} onChange={(e) => setUsername(e.target.value)} placeholder="用户名" required />
          <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder="密码（≥8 位）" required />
          <Select value={role} onValueChange={setRole}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="viewer">观察者</SelectItem>
              <SelectItem value="admin">管理员</SelectItem>
            </SelectContent>
          </Select>
          <Select value={groupIds.join(',')} onValueChange={(v) => setGroupIds(v ? v.split(',').map(Number) : [])}>
            <SelectTrigger><SelectValue placeholder="授权分组" /></SelectTrigger>
            <SelectContent>
              <SelectItem value="">无</SelectItem>
              {groups.map((g) => <SelectItem key={g.id} value={String(g.id)}>{g.name}</SelectItem>)}
            </SelectContent>
          </Select>
          <Button type="submit">添加</Button>
        </form>
        <p className="min-h-5 text-sm text-destructive">{error}</p>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>用户名</TableHead>
              <TableHead>角色</TableHead>
              <TableHead>2FA</TableHead>
              <TableHead className="w-10"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {users.map((u) => (
              <TableRow key={u.id}>
                <TableCell className="font-medium">{u.username}</TableCell>
                <TableCell>{u.role === 'admin' ? '管理员' : '观察者'}</TableCell>
                <TableCell>{u.totp_enabled ? '已启用' : '未启用'}</TableCell>
                <TableCell>
                  <div className="flex justify-end gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      title="重置密码"
                      onClick={() => {
                        setResetUser(u)
                        setNewPass('')
                        setResetMsg('')
                      }}
                    >
                      <KeyRound className="size-4 text-muted-foreground" />
                    </Button>
                    <Button variant="ghost" size="icon" title="删除" onClick={() => setDelUser(u)}>
                      <Trash2 className="size-4 text-muted-foreground" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        {delUser && (
          <ConfirmDialog
            open
            onOpenChange={(v) => !v && setDelUser(null)}
            title={`删除用户 ${delUser.username}？`}
            description="该账号将立即退出并无法登录。"
            onConfirm={async () => {
              await fetch(`/api/admin/users/${delUser.id}`, { method: 'DELETE' })
              setDelUser(null)
              load()
            }}
          />
        )}
        {resetUser && (
          <Dialog open onOpenChange={(v) => !v && setResetUser(null)}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>重置 {resetUser.username} 的密码</DialogTitle>
                <DialogDescription>
                  重置后该账号全部会话立即下线，需用新密码重新登录。
                </DialogDescription>
              </DialogHeader>
              <div className="space-y-2">
                <Input
                  type="password"
                  value={newPass}
                  onChange={(e) => setNewPass(e.target.value)}
                  placeholder="新密码（≥8 位）"
                  autoFocus
                />
                {resetMsg && <p className="text-sm text-destructive">{resetMsg}</p>}
              </div>
              <DialogFooter>
                <Button
                  onClick={async () => {
                    if (newPass.length < 8) {
                      setResetMsg('新密码至少 8 位')
                      return
                    }
                    const res = await fetch(`/api/admin/users/${resetUser.id}/reset-password`, {
                      method: 'POST',
                      headers: { 'Content-Type': 'application/json' },
                      body: JSON.stringify({ new_password: newPass, old_password: '' }),
                    })
                    if (!res.ok) {
                      const d = await res.json().catch(() => ({}))
                      setResetMsg(d.error ?? '重置失败')
                      return
                    }
                    setResetUser(null)
                  }}
                >
                  重置
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        )}
      </CardContent>
    </Card>
  )
}

// ---------- 审计 ----------

function AuditTab() {
  const [logs, setLogs] = useState<AuditLog[]>([])
  useEffect(() => {
    fetch('/api/admin/audit').then((r) => r.json()).then((d) => setLogs(d.logs ?? []))
  }, [])
  return (
    <Card className="py-4">
      <CardHeader><CardTitle className="text-base">审计日志（最近 200 条）</CardTitle></CardHeader>
      <CardContent>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>时间</TableHead>
              <TableHead>用户</TableHead>
              <TableHead>操作</TableHead>
              <TableHead className="hidden sm:table-cell">详情</TableHead>
              <TableHead className="hidden md:table-cell">IP</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {logs.map((l) => (
              <TableRow key={l.id}>
                <TableCell className="text-xs text-muted-foreground">
                  {new Date(l.at * 1000).toLocaleString('zh-CN')}
                </TableCell>
                <TableCell>{l.user ?? '—'}</TableCell>
                <TableCell>{l.action}</TableCell>
                <TableCell className="hidden max-w-40 truncate text-xs sm:table-cell">{l.detail}</TableCell>
                <TableCell className="hidden text-xs text-muted-foreground md:table-cell">{l.ip ?? ''}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  )
}


// ---------- 拨测任务管理 ----------

interface DialTaskRow { id: number; kind: string; target: string; interval_secs: number; enabled: boolean }

function DialDialog({ server, onClose }: { server: ServerView; onClose: () => void }) {
  const [tasks, setTasks] = useState<DialTaskRow[]>([])
  const [kind, setKind] = useState('http')
  const [target, setTarget] = useState('')
  const [interval, setInterval] = useState('60')
  const [error, setError] = useState('')

  const load = useCallback(() => {
    fetch(`/api/admin/servers/${server.id}/dial-tasks`).then((r) => r.json()).then((d) => setTasks(d.tasks ?? []))
  }, [server.id])
  useEffect(load, [load])

  const add = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    if (!target.trim()) return
    const res = await fetch(`/api/admin/servers/${server.id}/dial-tasks`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ kind, target: target.trim(), interval_secs: Number(interval) || 60 }),
    })
    if (!res.ok) {
      setError((await res.json()).error ?? '添加失败')
      return
    }
    setTarget('')
    load()
  }

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>拨测任务 · {server.name}</DialogTitle>
          <DialogDescription>由该服务器主动发起的受约束检查（HTTP GET / TCP 连通），间隔最少 10 秒</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          {tasks.map((t) => (
            <div key={t.id} className="flex items-center gap-2 border-b pb-1.5 text-sm last:border-0">
              <Badge variant="outline" className="font-mono text-xs">{t.kind.toUpperCase()}</Badge>
              <span className="min-w-0 flex-1 truncate font-mono text-xs">{t.target}</span>
              <span className="text-xs text-muted-foreground">{t.interval_secs}s</span>
              <Button variant="ghost" size="icon" onClick={async () => {
                await fetch(`/api/admin/dial-tasks/${t.id}`, { method: 'DELETE' })
                load()
              }}>
                <Trash2 className="size-4 text-muted-foreground" />
              </Button>
            </div>
          ))}
          <form onSubmit={add} className="grid grid-cols-5 gap-2">
            <Select value={kind} onValueChange={setKind}>
              <SelectTrigger className="col-span-1"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="http">HTTP</SelectItem>
                <SelectItem value="tcp">TCP</SelectItem>
              </SelectContent>
            </Select>
            <Input value={target} onChange={(e) => setTarget(e.target.value)} placeholder={kind === 'http' ? 'https://example.com' : 'example.com:443'} className="col-span-3 font-mono text-xs" />
            <Input value={interval} onChange={(e) => setInterval(e.target.value)} type="number" min={10} title="间隔秒" className="col-span-1" />
            <Button type="submit" className="col-span-5" size="sm"><Plus className="size-4" /> 添加任务</Button>
          </form>
          <p className="min-h-5 text-sm text-destructive">{error}</p>
        </div>
      </DialogContent>
    </Dialog>
  )
}


// ---------- 进程监控规则 ----------

interface ProcRuleRow { id: number; name: string; exact: boolean; display_name: string }

function ProcDialog({ server, onClose }: { server: ServerView; onClose: () => void }) {
  const [rules, setRules] = useState<ProcRuleRow[]>([])
  const [name, setName] = useState('')
  const [exact, setExact] = useState(true)
  const [error, setError] = useState('')

  const load = useCallback(() => {
    fetch(`/api/admin/servers/${server.id}/process-rules`).then((r) => r.json()).then((d) => setRules(d.rules ?? []))
  }, [server.id])
  useEffect(load, [load])

  const add = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    if (!name.trim()) return
    const res = await fetch(`/api/admin/servers/${server.id}/process-rules`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name.trim(), exact }),
    })
    if (!res.ok) {
      setError((await res.json()).error ?? '添加失败')
      return
    }
    setName('')
    load()
  }

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>进程监控 · {server.name}</DialogTitle>
          <DialogDescription>按进程名匹配并统计实例数 / CPU / 内存（只读，无控制语义）</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          {rules.map((r) => (
            <div key={r.id} className="flex items-center gap-2 border-b pb-1.5 text-sm last:border-0">
              <span className="min-w-0 flex-1 truncate font-mono text-xs">{r.name}</span>
              <Badge variant="outline" className="text-xs">{r.exact ? '精确' : '包含'}</Badge>
              <Button variant="ghost" size="icon" onClick={async () => {
                await fetch(`/api/admin/process-rules/${r.id}`, { method: 'DELETE' })
                load()
              }}>
                <Trash2 className="size-4 text-muted-foreground" />
              </Button>
            </div>
          ))}
          <form onSubmit={add} className="flex gap-2">
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="进程名，如 nginx" className="font-mono text-xs" />
            <Button variant={exact ? 'default' : 'outline'} size="sm" type="button" onClick={() => setExact(!exact)}>
              {exact ? '精确' : '包含'}
            </Button>
            <Button type="submit" size="sm"><Plus className="size-4" /></Button>
          </form>
          <p className="min-h-5 text-sm text-destructive">{error}</p>
        </div>
      </DialogContent>
    </Dialog>
  )
}


// ---------- 站点设置 ----------

// 常用地区（国旗 emoji + 名称，存整串作为徽章）
export const REGIONS = [
  '🇨🇳 中国大陆', '🇭🇰 香港', '🇹🇼 台湾', '🇯🇵 日本', '🇰🇷 韩国', '🇸🇬 新加坡',
  '🇲🇾 马来西亚', '🇹🇭 泰国', '🇺🇸 美国', '🇨🇦 加拿大', '🇬🇧 英国', '🇩🇪 德国',
  '🇫🇷 法国', '🇳🇱 荷兰', '🇦🇺 澳大利亚', '🇮🇳 印度', '🇹🇷 土耳其', '🇷🇺 俄罗斯', '🇧🇷 巴西',
]

function SettingsTab() {
  const [siteName, setSiteName] = useState('')
  const [description, setDescription] = useState('')
  const [logo, setLogo] = useState('')
  const [logoBusy, setLogoBusy] = useState(false)
  const [publicEnabled, setPublicEnabled] = useState(true)
  const [globeEnabled, setGlobeEnabled] = useState(true)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState('')
  const [loaded, setLoaded] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    fetch('/api/admin/settings')
      .then((r) => r.json())
      .then((d) => {
        if (d.error) return
        setSiteName(d.site_name ?? '')
        setDescription(d.site_description ?? '')
        setLogo(d.site_logo ?? '')
        setPublicEnabled(d.public_page_enabled !== false)
        setGlobeEnabled(d.globe_enabled !== false)
        setLoaded(true)
      })
  }, [])

  // LOGO 上传 / 恢复默认：立即生效并广播（顶栏/标签页图标同步换）
  const uploadLogo = async (f: File) => {
    setLogoBusy(true)
    setError('')
    const fd = new FormData()
    fd.append('file', f)
    const res = await fetch('/api/admin/logo', { method: 'POST', body: fd })
    const d = await res.json().catch(() => ({}))
    setLogoBusy(false)
    if (!res.ok) {
      setError(d.error ?? '上传失败')
      return
    }
    setLogo(d.logo)
    dispatchEvent(new CustomEvent('zt-site-changed'))
  }
  const resetLogo = async () => {
    setLogoBusy(true)
    const res = await fetch('/api/admin/logo', { method: 'DELETE' })
    setLogoBusy(false)
    if (!res.ok) {
      setError('恢复失败')
      return
    }
    setLogo('')
    dispatchEvent(new CustomEvent('zt-site-changed'))
  }

  const save = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setSaved(false)
    const res = await fetch('/api/admin/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        site_name: siteName,
        site_description: description,
        public_page_enabled: publicEnabled,
        globe_enabled: globeEnabled,
      }),
    })
    if (!res.ok) {
      setError((await res.json()).error ?? '保存失败')
      return
    }
    setSaved(true)
    // 立即刷新标题，免手动刷新页面
    document.title = `${siteName} · 服务器探针`
    dispatchEvent(new CustomEvent('zt-site-changed'))
    setTimeout(() => setSaved(false), 2000)
  }

  if (!loaded) return <div className="py-8 text-center text-muted-foreground">加载中…</div>
  return (
    <Card className="py-4">
      <CardHeader><CardTitle className="text-base">站点设置</CardTitle></CardHeader>
      <CardContent>
        <form onSubmit={save} className="max-w-lg space-y-4">
          <div className="space-y-2">
            <Label>站点 LOGO</Label>
            <div className="flex items-center gap-4">
              <img
                src={logo || '/favicon.svg'}
                alt="LOGO 预览"
                className="size-14 shrink-0 rounded-2xl border border-border object-cover"
              />
              <div className="min-w-0 space-y-1.5">
                <input
                  ref={fileRef}
                  type="file"
                  accept="image/png,image/jpeg,image/webp,image/gif,image/x-icon"
                  className="hidden"
                  onChange={(e) => {
                    const f = e.target.files?.[0]
                    e.target.value = '' // 同名文件可重复选择
                    if (f) uploadLogo(f)
                  }}
                />
                <div className="flex flex-wrap gap-2">
                  <Button type="button" variant="outline" size="sm" disabled={logoBusy} onClick={() => fileRef.current?.click()}>
                    {logoBusy ? '处理中…' : '上传图片'}
                  </Button>
                  {logo && (
                    <Button type="button" variant="ghost" size="sm" disabled={logoBusy} onClick={resetLogo}>
                      恢复默认
                    </Button>
                  )}
                </div>
                <p className="text-xs text-muted-foreground">
                  显示在顶栏与浏览器标签页（png / jpg / webp / gif / ico，≤1MB）
                </p>
              </div>
            </div>
          </div>
          <div className="space-y-2">
            <Label>站点名称</Label>
            <Input value={siteName} onChange={(e) => setSiteName(e.target.value)} placeholder="如 我的探针站" maxLength={32} />
            <p className="text-xs text-muted-foreground">显示在页面标题、顶栏、登录页（1-32 字符）</p>
          </div>
          <div className="space-y-2">
            <Label>站点描述（SEO）</Label>
            <Input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="网页 meta description" maxLength={160} />
            <p className="text-xs text-muted-foreground">写入 &lt;meta name=description&gt;，供搜索引擎摘要（≤160 字符）</p>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-border p-3">
            <div>
              <div className="text-sm font-medium">节点世界地图</div>
              <div className="text-xs text-muted-foreground">
                仪表盘概览下方的点亮地图与地区列表（关闭后不再显示）
              </div>
            </div>
            <Switch checked={globeEnabled} onCheckedChange={setGlobeEnabled} />
          </div>
          <div className="flex items-center justify-between rounded-lg border border-border p-3">
            <div>
              <div className="text-sm font-medium">公开展示页</div>
              <div className="text-xs text-muted-foreground">
                关闭后 /public 与其数据 API 对外不可用（面板本身始终需要登录）
              </div>
            </div>
            <Switch checked={publicEnabled} onCheckedChange={setPublicEnabled} />
          </div>
          {/* 状态行恒占一行高：提示出现/消失不把按钮顶得跳动 */}
          <div className="flex min-h-5 items-center text-sm">
            {error ? (
              <p className="text-destructive">{error}</p>
            ) : saved ? (
              <p className="text-emerald-600">已保存</p>
            ) : null}
          </div>
          <Button type="submit">保存设置</Button>
        </form>
      </CardContent>
    </Card>
  )
}


// ---------- 关于（版本 + 在线更新 + 更新源） ----------

function UpdateTab() {
  const [info, setInfo] = useState<{ current: string; latest: string | null; target: string | null; base: string } | null>(null)
  const [version, setVersion] = useState('')
  const [checking, setChecking] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [done, setDone] = useState(false)
  // 更新源（原在站点设置；GitHub 直连不稳的国内环境在这里改镜像前缀）
  const [updateBase, setUpdateBase] = useState('')
  const [baseSaved, setBaseSaved] = useState(false)

  const check = useCallback(async () => {
    setChecking(true)
    setError('')
    const res = await fetch('/api/admin/update/check')
    const d = await res.json().catch(() => ({}))
    setChecking(false)
    if (!res.ok) {
      setError(d.error ?? '检查失败')
      return
    }
    setInfo(d)
    if (d.latest) setVersion(d.latest)
  }, [])
  useEffect(() => { check() }, [check])
  // check 顺带带回当前 update_base，填进输入框
  useEffect(() => {
    if (info?.base) setUpdateBase(info.base)
  }, [info?.base])

  const saveBase = async () => {
    const v = updateBase.trim()
    if (v && !v.startsWith('https://') && !v.startsWith('http://')) {
      setError('更新源必须以 http(s):// 开头')
      return
    }
    setError('')
    const res = await fetch('/api/admin/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ update_base: v }),
    })
    if (!res.ok) {
      const d = await res.json().catch(() => ({}))
      setError(d.error ?? '保存失败')
      return
    }
    setBaseSaved(true)
    setTimeout(() => setBaseSaved(false), 2000)
    check()
  }

  const run = async () => {
    setBusy(true)
    setError('')
    setDone(false)
    const res = await fetch('/api/admin/update/run', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ version }),
    })
    const d = await res.json().catch(() => ({}))
    if (!res.ok) {
      setBusy(false)
      setError(d.error ?? '更新失败')
      return
    }
    // 热切换中（exec 替换进程镜像，连接闪断属预期）：轮询直到版本号变化
    const want = version.replace(/^v/, '')
    const poll = setInterval(async () => {
      try {
        const r = await fetch('/api/status')
        if (!r.ok) return
        const s = await r.json()
        if (s.version === want) {
          clearInterval(poll)
          setBusy(false)
          setDone(true)
          // 前端资源随二进制一起换了，稍候自动整页刷新
          setTimeout(() => location.reload(), 1500)
        }
      } catch {
        /* 热切换瞬间断连，继续轮询 */
      }
    }, 3000)
    setTimeout(() => clearInterval(poll), 180000)
  }

  const hasNew = info?.latest && `v${info.current}` !== info.latest
  return (
    <Card className="py-4">
      <CardHeader><CardTitle className="text-base">关于</CardTitle></CardHeader>
      <CardContent className="max-w-lg space-y-4">
        <div className="grid grid-cols-2 gap-3 text-sm">
          <div className="rounded-lg border border-border p-3">
            <div className="text-xs text-muted-foreground">当前版本</div>
            <div className="tnum mt-1 font-medium">v{info?.current ?? '…'}</div>
          </div>
          <div className="rounded-lg border border-border p-3">
            <div className="text-xs text-muted-foreground">最新版本</div>
            <div className="tnum mt-1 font-medium">
              {checking ? '检查中…' : info ? (info.latest ?? '探测不到') : '…'}
            </div>
          </div>
        </div>

        {hasNew && (
          <p className="text-sm text-emerald-600">有新版本 {info.latest} 可用</p>
        )}
        {info && !info.latest && (
          <p className="text-xs text-muted-foreground">
            探测不到最新版本（更新源直回了下载内容或超时）——不影响更新，
            在下方输入要更新到的版本号即可（如 v0.2.3）
          </p>
        )}

        <div className="space-y-2">
          <Label>目标版本</Label>
          <div className="flex gap-2">
            <Input value={version} onChange={(e) => setVersion(e.target.value.trim())} placeholder="v0.2.3" className="tnum w-36" />
            <Button type="button" variant="outline" onClick={check} disabled={checking || busy}>
              {checking ? '检查中…' : '重新检查'}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            当前更新源：{info?.base ?? '…'}（在本页下方修改）
          </p>
        </div>

        {/* 按钮恒在（busy 时转"更新中…"），状态文字换到旁边——高度恒定不跳动 */}
        <div className="flex min-h-9 flex-wrap items-center gap-x-3 gap-y-1">
          <Button type="button" onClick={run} disabled={!version || checking || busy}>
            {busy ? '更新中…' : `更新到 ${version || '指定版本'}`}
          </Button>
          {busy && (
            <span className="text-sm text-muted-foreground">
              下载校验后自动热切换，服务中断几秒，完成后本页自动刷新
            </span>
          )}
          {done && !busy && <span className="text-sm text-emerald-600">✓ 已更新到 {version}，正在刷新…</span>}
          {error && !busy && <span className="text-sm text-destructive">{error}</span>}
        </div>
        <p className="text-xs text-muted-foreground">
          更新流程：下载 release 包 → sha256 校验 → 原子替换二进制 → 无缝热切换（进程不重启、数据不丢）；
          失败时旧版本继续运行。
        </p>

        <div className="space-y-2 border-t border-border pt-4">
          <Label>更新源</Label>
          <div className="flex gap-2">
            <Input
              value={updateBase}
              onChange={(e) => setUpdateBase(e.target.value)}
              placeholder="留空 = GitHub 官方 releases"
              className="min-w-0"
            />
            <Button type="button" variant="outline" onClick={saveBase}>
              {baseSaved ? '✓ 已保存' : '保存'}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            在线更新的下载地址。GitHub 直连不稳时可填镜像前缀，如
            https://ghfast.top/https://github.com/liaoweixiang2024-blip/zhengtang/releases
          </p>
        </div>
      </CardContent>
    </Card>
  )
}

// ---------- 服务器编辑（到期/购买/流量配额/重置日/分组/公开展示） ----------

const GB = 1024 ** 3

function EditServerDialog({
  server, groups, onClose, onSaved,
}: {
  server: ServerView
  groups: Group[]
  onClose: () => void
  onSaved: () => void
}) {
  const [name, setName] = useState(server.name)
  const [groupId, setGroupId] = useState<string>(server.group_id ? String(server.group_id) : 'none')
  const [expires, setExpires] = useState(server.expires_at ? dateStr(server.expires_at) : '')
  const [purchased, setPurchased] = useState(server.purchased_at ? dateStr(server.purchased_at) : '')
  const [quota, setQuota] = useState(server.traffic_quota_bytes ? String(server.traffic_quota_bytes / GB) : '')
  const [resetDay, setResetDay] = useState(server.traffic_reset_day ? String(server.traffic_reset_day) : 'natural')
  const [payCycle, setPayCycle] = useState(server.pay_cycle_months != null ? String(server.pay_cycle_months) : 'none')
  const [price, setPrice] = useState(server.price_per_cycle != null ? String(server.price_per_cycle) : '')
  const [currency, setCurrency] = useState(server.currency ?? 'CNY')
  const [region, setRegion] = useState(server.region ?? 'none')
  const [publicVisible, setPublicVisible] = useState(server.public_visible)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  const save = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setBusy(true)
    const toTs = (v: string) => (v ? Math.floor(new Date(v + 'T12:00:00Z').getTime() / 1000) : null)
    const quotaBytes = quota ? Math.round(parseFloat(quota) * GB) : null
    if (quota && (isNaN(quotaBytes!) || quotaBytes! <= 0)) {
      setError('流量配额格式错误')
      setBusy(false)
      return
    }
    const priceVal = price ? parseFloat(price) : null
    if (price && (priceVal == null || isNaN(priceVal) || priceVal < 0)) {
      setError('费用格式错误')
      setBusy(false)
      return
    }
    const body = {
      name: name.trim() || server.name,
      group_id: groupId === 'none' ? null : Number(groupId),
      expires_at: toTs(expires),
      purchased_at: toTs(purchased),
      traffic_quota_bytes: quotaBytes,
      traffic_reset_day: resetDay === 'natural' ? null : Number(resetDay),
      pay_cycle_months: payCycle === 'none' ? null : Number(payCycle),
      price_per_cycle: payCycle === 'none' || payCycle === '-1' ? null : priceVal,
      currency: payCycle === 'none' || payCycle === '-1' ? null : currency,
      region: region === 'none' ? null : region,
      public_visible: publicVisible,
    }
    const res = await fetch(`/api/admin/servers/${server.id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    setBusy(false)
    if (!res.ok) {
      setError((await res.json()).error ?? '保存失败')
      return
    }
    onSaved()
    onClose()
  }

  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>编辑服务器</DialogTitle>
          <DialogDescription>到期、购买、流量配额、账期与费用信息</DialogDescription>
        </DialogHeader>
        <form onSubmit={save} className="space-y-3">
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>名称</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>分组</Label>
              <Select value={groupId} onValueChange={setGroupId}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">未分组</SelectItem>
                  {groups.map((g) => <SelectItem key={g.id} value={String(g.id)}>{g.name}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>地区</Label>
              <Select
                value={region}
                onValueChange={async (v) => {
                  if (v === '__auto__') {
                    setError('')
                    const res = await fetch(`/api/admin/servers/${server.id}/detect-region`, { method: 'POST' })
                    const d = await res.json()
                    if (!res.ok) {
                      setError(d.error ?? '识别失败')
                      return
                    }
                    setRegion(d.region)
                    return
                  }
                  setRegion(v)
                }}
              >
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="__auto__">🔄 自动识别（按连接 IP）</SelectItem>
                  <SelectItem value="none">未设置</SelectItem>
                  {REGIONS.map((r) => (
                    <SelectItem key={r} value={r}>{r}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="min-h-4 text-xs text-destructive">{error}</p>
            </div>
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <Label>到期日</Label>
                <button
                  type="button"
                  className="text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                  onClick={() => setExpires('')}
                >
                  {expires ? '设为长期' : '长期有效 ✓'}
                </button>
              </div>
              <Input type="date" value={expires} onChange={(e) => setExpires(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>购买时间</Label>
              <Input type="date" value={purchased} onChange={(e) => setPurchased(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label>流量配额 (GB/月)</Label>
              <Input inputMode="decimal" value={quota} onChange={(e) => setQuota(e.target.value)} placeholder="留空=不限" />
            </div>
            <div className="space-y-1.5">
              <Label>流量重置日</Label>
              <Select value={resetDay} onValueChange={setResetDay}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="natural">按自然月（1 号）</SelectItem>
                  {Array.from({ length: 28 }, (_, i) => i + 1).map((d) => (
                    <SelectItem key={d} value={String(d)}>每月 {d} 日</SelectItem>
                  ))}
                  <SelectItem value="29">每月 29 日（2 月按月末）</SelectItem>
                  <SelectItem value="30">每月 30 日（2 月按月末）</SelectItem>
                  <SelectItem value="31">每月 31 日（小月按月末）</SelectItem>
                  <SelectItem value="99">每月月末（自动跟随当月最后一天）</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>付费周期</Label>
              <Select value={payCycle} onValueChange={setPayCycle}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">未填写</SelectItem>
                  {Object.entries(CYCLE_LABEL).map(([m, label]) => (
                    <SelectItem key={m} value={m}>{label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>每期费用</Label>
              <div className="flex gap-2">
                <Input
                  inputMode="decimal"
                  value={price}
                  onChange={(e) => setPrice(e.target.value)}
                  placeholder={payCycle === '-1' ? '免费' : '留空=不计'}
                  disabled={payCycle === 'none' || payCycle === '-1'}
                />
                <Select value={currency} onValueChange={setCurrency} disabled={payCycle === 'none' || payCycle === '-1'}>
                  <SelectTrigger className="w-24"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    {Object.keys(CURRENCY_SYMBOL).map((c) => (
                      <SelectItem key={c} value={c}>{c}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-border p-3">
            <span className="text-sm">公开展示（/public 匿名可见）</span>
            <Switch checked={publicVisible} onCheckedChange={setPublicVisible} />
          </div>
          {purchased && resetDay === 'natural' && (
            <p className="text-xs text-muted-foreground">
              提示：已填购买时间，可把重置日设为购买日对应的日子，流量将按购买日对齐的账期统计
            </p>
          )}
          <p className="min-h-5 text-sm text-destructive">{error}</p>
          <Button type="submit" className="w-full" disabled={busy}>
            {busy ? '保存中…' : '保存'}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  )
}
