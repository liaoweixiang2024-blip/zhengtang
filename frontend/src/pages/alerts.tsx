// 告警中心：事件列表 + 通知渠道管理
import { cn } from '@/lib/utils'
import { useCallback, useEffect, useState } from 'react'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Pencil, Plus, Send, Trash2 } from 'lucide-react'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'

interface AlertEvent { id: number; server: string; summary: string; fired_at: number; resolved_at: number | null }
interface Channel { id: number; type: string; name: string; config: Record<string, unknown>; enabled: boolean }

const CHANNEL_LABEL: Record<string, string> = { telegram: 'Telegram', bark: 'Bark', email: '邮件', webhook: 'Webhook' }

export function Alerts({ isAdmin }: { isAdmin: boolean }) {
  return (
    <Tabs defaultValue="events" className="space-y-4">
      <TabsList>
        <TabsTrigger value="events">告警事件</TabsTrigger>
        {isAdmin && <TabsTrigger value="channels">通知渠道</TabsTrigger>}
      </TabsList>
      <TabsContent value="events"><EventsTab /></TabsContent>
      {isAdmin && <TabsContent value="channels"><ChannelsTab /></TabsContent>}
    </Tabs>
  )
}

function EventsTab() {
  const [events, setEvents] = useState<AlertEvent[]>([])
  useEffect(() => {
    fetch('/api/alerts/events').then((r) => r.json()).then((d) => setEvents(d.events ?? []))
  }, [])
  return (
    <Card className="py-4">
      <CardHeader><CardTitle className="text-base">告警事件（最近 200 条）</CardTitle></CardHeader>
      <CardContent>
        {events.length === 0 ? (
          <p className="py-8 text-center text-muted-foreground">暂无告警</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>时间</TableHead>
                <TableHead>内容</TableHead>
                <TableHead>状态</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {events.map((e) => (
                <TableRow key={e.id}>
                  <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                    {new Date(e.fired_at * 1000).toLocaleString('zh-CN')}
                  </TableCell>
                  <TableCell>{e.summary}</TableCell>
                  <TableCell>
                    {e.resolved_at ? <Badge variant="secondary">已恢复</Badge> : <Badge variant="destructive">告警中</Badge>}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  )
}

function ChannelsTab() {
  const [channels, setChannels] = useState<Channel[]>([])
  const [editing, setEditing] = useState<Channel | 'new' | null>(null)
  const [delChannel, setDelChannel] = useState<Channel | null>(null)
  const [testing, setTesting] = useState<number | null>(null)
  const [testMsg, setTestMsg] = useState('')

  const load = useCallback(() => {
    fetch('/api/admin/channels').then((r) => r.json()).then((d) => setChannels(d.channels ?? []))
  }, [])
  useEffect(load, [load])

  const setEnabled = async (ch: Channel, enabled: boolean) => {
    setChannels((list) => list.map((c) => (c.id === ch.id ? { ...c, enabled } : c)))
    await fetch(`/api/admin/channels/${ch.id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled }),
    })
  }

  const test = async (id: number) => {
    setTesting(id)
    setTestMsg('')
    const res = await fetch(`/api/admin/channels/${id}/test`, { method: 'POST' })
    setTesting(null)
    setTestMsg(res.ok ? '✓ 测试消息已发送，请到对应渠道查收' : '✗ 发送失败（检查配置；服务端日志有详情）')
  }

  return (
    <Card className="py-4">
      <CardHeader className="flex-row items-center justify-between">
        <div>
          <CardTitle className="text-base">通知渠道</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">告警与恢复通知会推送到所有已启用的渠道</p>
        </div>
        <Button size="sm" onClick={() => setEditing('new')}><Plus className="size-4" /> 添加渠道</Button>
      </CardHeader>
      <CardContent className="space-y-3">
        <Table>
          <TableBody>
            {channels.map((ch) => (
              <TableRow key={ch.id} className={cn(!ch.enabled && 'opacity-50')}>
                <TableCell>
                  <Switch checked={ch.enabled} onCheckedChange={(v) => setEnabled(ch, v)} title={ch.enabled ? '已启用（点击停用）' : '已停用（点击启用）'} />
                </TableCell>
                <TableCell className="min-w-0 truncate font-medium">{ch.name}</TableCell>
                <TableCell><Badge variant="secondary">{CHANNEL_LABEL[ch.type] ?? ch.type}</Badge></TableCell>
                <TableCell className="w-32">
                  <div className="flex justify-end gap-1">
                    <Button variant="ghost" size="icon" title="编辑" onClick={() => setEditing(ch)}>
                      <Pencil className="size-4 text-muted-foreground" />
                    </Button>
                    <Button variant="ghost" size="icon" title="测试发送" disabled={testing === ch.id} onClick={() => test(ch.id)}>
                      <Send className={cn('size-4 text-muted-foreground', testing === ch.id && 'animate-pulse')} />
                    </Button>
                    <Button variant="ghost" size="icon" title="删除" onClick={() => setDelChannel(ch)}>
                      <Trash2 className="size-4 text-muted-foreground" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
        {channels.length === 0 && <p className="py-4 text-center text-muted-foreground">还没有渠道，点右上角「添加渠道」创建</p>}
        <p className="min-h-5 text-sm text-muted-foreground">{testMsg}</p>
        {editing && <ChannelDialog channel={editing === 'new' ? null : editing} onClose={() => setEditing(null)} onSaved={load} />}
        {delChannel && (
          <ConfirmDialog
            open
            onOpenChange={(v) => !v && setDelChannel(null)}
            title={`删除渠道 ${delChannel.name}？`}
            description="删除后告警不再推送到该渠道。"
            onConfirm={async () => {
              await fetch(`/api/admin/channels/${delChannel.id}`, { method: 'DELETE' })
              setDelChannel(null)
              load()
            }}
          />
        )}
      </CardContent>
    </Card>
  )
}

/** 各渠道的结构化字段（含说明）；config 仍是后端的 JSON，表单层负责翻译 */
const FIELDS: Record<string, { key: string; label: string; placeholder?: string; hint?: string; type?: string }[]> = {
  telegram: [
    { key: 'bot_token', label: 'Bot Token', placeholder: '123456:ABC-DEF…', hint: '找 @BotFather 创建 bot 获得' },
    { key: 'chat_id', label: 'Chat ID', placeholder: '-1001234567890', hint: '把 bot 拉进群后找 @userinfobot 查 ID（群为负数）' },
  ],
  bark: [
    { key: 'key', label: 'Device Key', placeholder: 'abcdefghijk', hint: 'Bark App 首页复制' },
    { key: 'server', label: '服务器（可选）', placeholder: 'https://api.day.app', hint: '自建 Bark 服务才填，默认官方' },
  ],
  email: [
    { key: 'smtp_host', label: 'SMTP 服务器', placeholder: 'smtp.example.com' },
    { key: 'smtp_port', label: '端口', placeholder: '465', type: 'number' },
    { key: 'username', label: '账号', placeholder: 'alert@example.com' },
    { key: 'password', label: '密码/授权码', type: 'password' },
    { key: 'to', label: '收件人', placeholder: 'you@example.com' },
  ],
  webhook: [
    { key: 'url', label: 'Webhook 地址', placeholder: 'https://example.com/hook', hint: '以 POST 收 {title, body} JSON' },
  ],
}

/** 新建/编辑渠道：按类型渲染结构化字段，不再让用户手写 JSON */
function ChannelDialog({ channel, onClose, onSaved }: { channel: Channel | null; onClose: () => void; onSaved: () => void }) {
  const [type, setType] = useState(channel?.type ?? 'telegram')
  const [name, setName] = useState(channel?.name ?? '')
  const [cfg, setCfg] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {}
    for (const f of FIELDS[channel?.type ?? 'telegram']) init[f.key] = String(channel?.config?.[f.key] ?? '')
    return init
  })
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  const switchType = (t: string) => {
    setType(t)
    const init: Record<string, string> = {}
    for (const f of FIELDS[t]) init[f.key] = String(channel?.config?.[f.key] ?? '')
    setCfg(init)
  }

  const submit = async () => {
    setError('')
    const config: Record<string, string | number> = {}
    for (const f of FIELDS[type]) {
      const v = (cfg[f.key] ?? '').trim()
      if (f.key === 'server' && !v) continue // bark 官方默认
      if (!v) {
        setError(`请填写 ${f.label}`)
        return
      }
      config[f.key] = f.type === 'number' ? Number(v) : v
    }
    setBusy(true)
    const res = await fetch(channel ? `/api/admin/channels/${channel.id}` : '/api/admin/channels', {
      method: channel ? 'PATCH' : 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(channel
        ? { name: name || CHANNEL_LABEL[type], config }
        : { type, name: name || CHANNEL_LABEL[type], config }),
    })
    setBusy(false)
    if (!res.ok) {
      setError((await res.json().catch(() => ({}))).error ?? '保存失败')
      return
    }
    onSaved()
    onClose()
  }

  return (
    <Dialog open onOpenChange={(v) => !busy && !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{channel ? `编辑 ${channel.name}` : '添加通知渠道'}</DialogTitle>
          <DialogDescription>告警触发时推送标题与摘要到该渠道。</DialogDescription>
        </DialogHeader>
        <div className="max-h-[60vh] space-y-3 overflow-y-auto">
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>类型</Label>
              <Select value={type} onValueChange={switchType} disabled={!!channel}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="telegram">Telegram</SelectItem>
                  <SelectItem value="bark">Bark (iOS)</SelectItem>
                  <SelectItem value="email">邮件</SelectItem>
                  <SelectItem value="webhook">Webhook</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>名称</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={CHANNEL_LABEL[type]} maxLength={32} />
            </div>
          </div>
          {FIELDS[type].map((f) => (
            <div key={f.key} className="space-y-1.5">
              <Label>{f.label}</Label>
              <Input
                type={f.type ?? 'text'}
                value={cfg[f.key] ?? ''}
                onChange={(e) => setCfg((c) => ({ ...c, [f.key]: e.target.value }))}
                placeholder={f.placeholder}
                autoComplete="off"
              />
              {f.hint && <p className="text-xs text-muted-foreground">{f.hint}</p>}
            </div>
          ))}
          <p className="min-h-5 text-sm text-destructive">{error}</p>
        </div>
        <DialogFooter>
          <Button onClick={submit} disabled={busy}>{busy ? '保存中…' : channel ? '保存' : '添加'}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
