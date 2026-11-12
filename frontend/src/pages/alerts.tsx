// 告警中心：事件列表 + 通知渠道管理
import { useCallback, useEffect, useState } from 'react'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Badge } from '@/components/ui/badge'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Plus, Send, Trash2 } from 'lucide-react'

interface AlertEvent { id: number; server: string; summary: string; fired_at: number; resolved_at: number | null }
interface Channel { id: number; type: string; name: string }

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
  const [type, setType] = useState('telegram')
  const [name, setName] = useState('')
  // 各类型配置字段（JSON 文本域，按类型给占位模板）
  const [config, setConfig] = useState('')
  const [error, setError] = useState('')
  const [delChannel, setDelChannel] = useState<Channel | null>(null)

  const placeholder: Record<string, string> = {
    telegram: '{"bot_token":"123:abc","chat_id":"-100123"}',
    bark: '{"key":"your_bark_key"}',
    webhook: '{"url":"https://example.com/hook"}',
    email: '{"smtp_host":"smtp.example.com","smtp_port":465,"username":"a@b.c","password":"***","to":"you@b.c"}',
  }

  const load = useCallback(() => {
    fetch('/api/admin/channels').then((r) => r.json()).then((d) => setChannels(d.channels ?? []))
  }, [])
  useEffect(load, [load])

  const add = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    try {
      JSON.parse(config || placeholder[type])
    } catch {
      setError('配置必须是合法 JSON')
      return
    }
    const res = await fetch('/api/admin/channels', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ type, name: name || CHANNEL_LABEL[type], config: JSON.parse(config || placeholder[type]) }),
    })
    if (!res.ok) {
      setError((await res.json()).error ?? '添加失败')
      return
    }
    setName('')
    setConfig('')
    load()
  }

  const test = async (id: number) => {
    const res = await fetch(`/api/admin/channels/${id}/test`, { method: 'POST' })
    alert(res.ok ? '已发送，请查收' : '发送失败，查看服务端日志')
  }

  return (
    <Card className="py-4">
      <CardHeader><CardTitle className="text-base">通知渠道</CardTitle></CardHeader>
      <CardContent className="space-y-4">
        <form onSubmit={add} className="grid gap-2 md:grid-cols-4">
          <Select value={type} onValueChange={setType}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="telegram">Telegram</SelectItem>
              <SelectItem value="bark">Bark (iOS)</SelectItem>
              <SelectItem value="email">邮件</SelectItem>
              <SelectItem value="webhook">Webhook</SelectItem>
            </SelectContent>
          </Select>
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="名称（可空）" />
          <Input value={config} onChange={(e) => setConfig(e.target.value)} placeholder={placeholder[type]} className="md:col-span-2 font-mono text-xs" />
          <Button type="submit"><Plus className="size-4" /> 添加</Button>
        </form>
        {error && <p className="text-sm text-destructive">{error}</p>}
        <Table>
          <TableBody>
            {channels.map((ch) => (
              <TableRow key={ch.id}>
                <TableCell className="font-medium">{ch.name}</TableCell>
                <TableCell className="text-muted-foreground">{CHANNEL_LABEL[ch.type] ?? ch.type}</TableCell>
                <TableCell className="w-24">
                  <div className="flex gap-1">
                    <Button variant="ghost" size="icon" title="测试发送" onClick={() => test(ch.id)}>
                      <Send className="size-4 text-muted-foreground" />
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
        {channels.length === 0 && <p className="text-center text-muted-foreground">还没有渠道，添加后告警将推送到所有渠道</p>}
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
