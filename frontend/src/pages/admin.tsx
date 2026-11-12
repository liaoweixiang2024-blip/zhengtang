// 管理页：服务器管理 / 用户 / 分组 / 审计日志
import { useCallback, useEffect, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { fmtBytes } from '@/lib/format'
import type { ServerView } from '@/lib/types'
import { ListTree, Plus, Radar, Trash2 } from 'lucide-react'

interface Group { id: number; name: string }
interface User { id: number; username: string; role: string; totp_enabled: boolean }
interface AuditLog { id: number; user: string | null; action: string; detail: string; ip: string | null; at: number }

export function Admin() {
  return (
    <Tabs defaultValue="servers" className="space-y-4">
      <TabsList>
        <TabsTrigger value="servers">服务器</TabsTrigger>
        <TabsTrigger value="groups">分组</TabsTrigger>
        <TabsTrigger value="users">用户</TabsTrigger>
        <TabsTrigger value="audit">审计日志</TabsTrigger>
      </TabsList>
      <TabsContent value="servers"><ServersTab /></TabsContent>
      <TabsContent value="groups"><GroupsTab /></TabsContent>
      <TabsContent value="users"><UsersTab /></TabsContent>
      <TabsContent value="audit"><AuditTab /></TabsContent>
    </Tabs>
  )
}

// ---------- 服务器管理 ----------

function ServersTab() {
  const [servers, setServers] = useState<ServerView[]>([])
  const [groups, setGroups] = useState<Group[]>([])
  const [addOpen, setAddOpen] = useState(false)
  const [installCmd, setInstallCmd] = useState<string | null>(null)
  const [dialServer, setDialServer] = useState<ServerView | null>(null)
  const [procServer, setProcServer] = useState<ServerView | null>(null)

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
    setInstallCmd(d.install_cmd ?? '生成失败')
    load()
  }

  const update = async (id: string, patch: Record<string, unknown>) => {
    await fetch(`/api/admin/servers/${id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(patch),
    })
    load()
  }

  const remove = async (id: string, name: string) => {
    if (!confirm(`确定删除 ${name}？其历史数据将一并删除`)) return
    await fetch(`/api/admin/servers/${id}`, { method: 'DELETE' })
    load()
  }

  return (
    <Card className="py-4">
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle className="text-base">服务器（{servers.length}）</CardTitle>
        <AddServerDialog open={addOpen} onOpenChange={setAddOpen} onAdd={add} installCmd={installCmd} onCopy={() => setInstallCmd(null)} />
        {dialServer && <DialDialog server={dialServer} onClose={() => setDialServer(null)} />}
        {procServer && <ProcDialog server={procServer} onClose={() => setProcServer(null)} />}
      </CardHeader>
      <CardContent>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>名称</TableHead>
              <TableHead>分组</TableHead>
              <TableHead>到期日</TableHead>
              <TableHead className="hidden sm:table-cell">流量配额</TableHead>
              <TableHead>公开展示</TableHead>
              <TableHead className="w-10">拨测</TableHead>
              <TableHead className="w-10">进程</TableHead>
              <TableHead className="w-10"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {servers.map((s) => (
              <TableRow key={s.id}>
                <TableCell className="font-medium">{s.name}</TableCell>
                <TableCell>
                  <Select
                    value={s.group_id ? String(s.group_id) : 'none'}
                    onValueChange={(v) => update(s.id, { group_id: v === 'none' ? null : Number(v) })}
                  >
                    <SelectTrigger className="h-7 w-28 text-xs"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">未分组</SelectItem>
                      {groups.map((g) => <SelectItem key={g.id} value={String(g.id)}>{g.name}</SelectItem>)}
                    </SelectContent>
                  </Select>
                </TableCell>
                <TableCell>
                  <DateCell
                    value={s.expires_at ? new Date(s.expires_at * 1000).toISOString().slice(0, 10) : ''}
                    onChange={(v) => update(s.id, { expires_at: v ? Math.floor(new Date(v + 'T12:00:00Z').getTime() / 1000) : null })}
                  />
                </TableCell>
                <TableCell className="hidden sm:table-cell">
                  <span className="text-xs text-muted-foreground">
                    {s.traffic_quota_bytes ? fmtBytes(s.traffic_quota_bytes) : '不限'}
                  </span>
                </TableCell>
                <TableCell>
                  <Switch checked={s.public_visible} onCheckedChange={(v) => update(s.id, { public_visible: v })} />
                </TableCell>
                <TableCell>
                  <Button variant="ghost" size="icon" title="拨测任务" onClick={() => setDialServer(s)}>
                    <Radar className="size-4 text-muted-foreground" />
                  </Button>
                </TableCell>
                <TableCell>
                  <Button variant="ghost" size="icon" title="进程监控" onClick={() => setProcServer(s)}>
                    <ListTree className="size-4 text-muted-foreground" />
                  </Button>
                </TableCell>
                <TableCell>
                  <Button variant="ghost" size="icon" onClick={() => remove(s.id, s.name)}>
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

function DateCell({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <Input
      type="date"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="h-7 w-36 text-xs"
    />
  )
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
              ? '安装命令已生成（私钥仅此一次显示），在目标服务器上执行：'
              : '输入名称生成 Agent 安装参数'}
          </DialogDescription>
        </DialogHeader>
        {installCmd ? (
          <div className="space-y-2">
            <pre className="max-h-40 overflow-auto rounded-md bg-muted p-3 text-xs break-all whitespace-pre-wrap">{installCmd}</pre>
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
                  <Button variant="ghost" size="icon" onClick={async () => {
                    if (confirm(`删除分组 ${g.name}？`)) {
                      await fetch(`/api/admin/groups/${g.id}`, { method: 'DELETE' })
                      load()
                    }
                  }}>
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

// ---------- 用户 ----------

function UsersTab() {
  const [users, setUsers] = useState<User[]>([])
  const [groups, setGroups] = useState<Group[]>([])
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [role, setRole] = useState('viewer')
  const [groupIds, setGroupIds] = useState<number[]>([])
  const [error, setError] = useState('')

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
        {error && <p className="text-sm text-destructive">{error}</p>}
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
                  <Button variant="ghost" size="icon" onClick={async () => {
                    if (confirm(`删除用户 ${u.username}？`)) {
                      await fetch(`/api/admin/users/${u.id}`, { method: 'DELETE' })
                      load()
                    }
                  }}>
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
          {error && <p className="text-sm text-destructive">{error}</p>}
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
          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>
      </DialogContent>
    </Dialog>
  )
}
