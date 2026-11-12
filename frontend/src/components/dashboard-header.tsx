// 概览统计卡片行 + 分组标签 + 形态切换（参考 JIKASEI 主题交互）
import { useEffect, useState } from 'react'
import { Card, CardContent } from '@/components/ui/card'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { fmtBytes, fmtSpeed } from '@/lib/format'
import { CURRENCY_SYMBOL, monthlyCostByCurrency, remainingValueByCurrency } from '@/lib/cost'
import type { ServerView } from '@/lib/types'
import { cn } from '@/lib/utils'
import { Check, Grid2x2, LayoutGrid, Rows3 } from 'lucide-react'

export type ViewMode = 'detailed' | 'compact' | 'mini'

const VIEW_LABEL: Record<ViewMode, string> = {
  detailed: '详细',
  compact: '紧凑',
  mini: '简约',
}

/** 概览卡：节点 / 月度预算 / 剩余价值 / 最忙节点 / 今日流量 / 总流量 / 实时网速 */
export function StatCards({ servers }: { servers: ServerView[] }) {
  // 今日流量（本地 0 点起，30s 轮询）
  const [today, setToday] = useState<{ in: number; out: number } | null>(null)
  useEffect(() => {
    const load = () => {
      const since = Math.floor(new Date(new Date().setHours(0, 0, 0, 0)).getTime() / 1000)
      fetch(`/api/traffic/summary?since=${since}`)
        .then((r) => (r.ok ? r.json() : null))
        .then((d) => d && setToday({ in: d.in ?? 0, out: d.out ?? 0 }))
        .catch(() => {})
    }
    load()
    const t = setInterval(load, 30_000)
    return () => clearInterval(t)
  }, [])

  const total = servers.length
  const online = servers.filter((s) => s.online).length
  const offline = total - online

  // 月度预算 / 剩余价值（取合计最大的货币为主货币）
  const budgets = monthlyCostByCurrency(servers)
  const budget = budgets.sort((a, b) => b.monthly - a.monthly)[0]
  const values = remainingValueByCurrency(servers)
  const remain = values.find((v) => v.currency === budget?.currency) ?? values.sort((a, b) => b.value - a.value)[0]
  const billed = servers.filter((s) => s.price_per_cycle != null && s.pay_cycle_months && s.pay_cycle_months > 0).length
  const unexpired = servers.filter((s) => s.expires_at && s.expires_at > Date.now() / 1000).length

  // 最忙节点 = 在线机器中 CPU 最高
  const busiest = servers
    .filter((s) => s.online && s.metrics)
    .sort((a, b) => (b.metrics!.cpu_usage ?? 0) - (a.metrics!.cpu_usage ?? 0))[0]

  const sum = (f: (m: NonNullable<ServerView['metrics']>) => number) =>
    servers.reduce((acc, s) => acc + (s.metrics ? f(s.metrics) : 0), 0)
  const totalIn = sum((m) => m.net_in_total)
  const totalOut = sum((m) => m.net_out_total)
  const speedIn = sum((m) => m.net_in_speed)
  const speedOut = sum((m) => m.net_out_speed)

  const sym = budget ? CURRENCY_SYMBOL[budget.currency] ?? budget.currency : ''

  const items = [
    {
      label: '节点',
      value: `${online}/${total}`,
      sub: offline > 0 ? `${offline} 台离线` : '全部在线',
      valueCls: offline > 0 ? 'text-amber-500' : 'text-emerald-500',
    },
    budget
      ? {
          label: '月度预算',
          value: `≈${sym}${budget.monthly.toFixed(2)}`,
          sub: `${billed} 台计费`,
        }
      : { label: '月度预算', value: '—', sub: '暂无计费信息' },
    remain && remain.value > 0
      ? {
          label: '剩余价值',
          value: `≈${CURRENCY_SYMBOL[remain.currency] ?? remain.currency}${remain.value.toFixed(2)}`,
          sub: `${unexpired} 台未到期`,
        }
      : { label: '剩余价值', value: '—', sub: '暂无到期机器' },
    busiest && busiest.metrics
      ? {
          label: '最忙节点',
          value: `${busiest.metrics.cpu_usage.toFixed(1)}%`,
          sub: busiest.name,
        }
      : { label: '最忙节点', value: '—', sub: '暂无在线节点' },
    { label: '今日流量', value: fmtBytes(today?.in ?? 0), sub: `↑ ${fmtBytes(today?.out ?? 0)}` },
    { label: '总流量', value: fmtBytes(totalIn), sub: `↑ ${fmtBytes(totalOut)}` },
    { label: '实时网速', value: `↓ ${fmtSpeed(speedIn)}`, sub: `↑ ${fmtSpeed(speedOut)}` },
  ]

  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-4 xl:grid-cols-7">
      {items.map((it) => (
        <Card key={it.label} className="py-3">
          <CardContent className="space-y-1 px-4">
            <div className="text-xs text-muted-foreground">{it.label}</div>
            <div className={cn('truncate text-lg font-semibold tabular-nums', (it as { valueCls?: string }).valueCls)} title={it.value}>
              {it.value}
            </div>
            <div className="truncate text-xs text-muted-foreground" title={it.sub}>{it.sub}</div>
          </CardContent>
        </Card>
      ))}
    </div>
  )
}

/** 分组标签行：全部 + 各组（组名目前由后端 group_id 提供，M4 管理页可命名） */
export function GroupTabs({
  groups, active, onChange,
}: {
  groups: { id: number; name: string; count: number }[]
  active: number | 'all'
  onChange: (g: number | 'all') => void
}) {
  const tabs = [{ id: 'all' as const, name: '全部', count: groups.reduce((a, g) => a + g.count, 0) }, ...groups]
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {tabs.map((t) => (
        <button
          key={String(t.id)}
          onClick={() => onChange(t.id)}
          className={cn(
            'rounded-full border px-3 py-1 text-xs transition-colors',
            active === t.id
              ? 'border-primary bg-primary text-primary-foreground'
              : 'text-muted-foreground hover:bg-accent hover:text-foreground',
          )}
        >
          {t.name}
          <span className="ml-1 opacity-70">{t.count}</span>
        </button>
      ))}
    </div>
  )
}

/** 卡片形态切换（localStorage 记住偏好） */
export function ViewModeToggle({ value, onChange }: { value: ViewMode; onChange: (v: ViewMode) => void }) {
  const icon = value === 'compact' ? <Rows3 className="size-[18px]" /> : value === 'mini' ? <Grid2x2 className="size-[18px]" /> : <LayoutGrid className="size-[18px]" />
  return (
    <DropdownMenu>
      <DropdownMenuTrigger title="卡片形态" className="inline-flex size-8 items-center justify-center rounded-full text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/40 data-[state=open]:bg-accent data-[state=open]:text-foreground">
        {icon}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {(Object.keys(VIEW_LABEL) as ViewMode[]).map((m) => (
          <DropdownMenuItem key={m} onClick={() => onChange(m)} className="gap-2">
            <Check className={cn('size-4', value !== m && 'invisible')} />
            {VIEW_LABEL[m]}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
