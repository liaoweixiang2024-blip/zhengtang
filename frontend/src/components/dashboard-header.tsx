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
import { AlignJustify, Check, GalleryVerticalEnd, Grid2x2 } from 'lucide-react'

export type ViewMode = 'detailed' | 'compact' | 'mini'

const VIEW_LABEL: Record<ViewMode, string> = {
  detailed: '详细',
  compact: '紧凑',
  mini: '简约',
}

/** 概览卡：节点 / 月度预算 / 剩余价值 / 最忙节点 / 今日流量 / 总流量 / 实时网速
 *  每张卡悬停弹出每台机器的明细列表 */
export function StatCards({ servers }: { servers: ServerView[] }) {
  // 今日流量（本地 0 点起，30s 轮询）
  const [today, setToday] = useState<{ in: number; out: number; per_server: { name: string; in: number; out: number }[] } | null>(null)
  useEffect(() => {
    const load = () => {
      const since = Math.floor(new Date(new Date().setHours(0, 0, 0, 0)).getTime() / 1000)
      fetch(`/api/traffic/summary?since=${since}`)
        .then((r) => (r.ok ? r.json() : null))
        .then((d) => d && setToday({ in: d.in ?? 0, out: d.out ?? 0, per_server: d.per_server ?? [] }))
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
  const nowSec = Date.now() / 1000
  const r2 = (n: number) => n.toFixed(2)

  const items: {
    label: string
    value: string
    sub: string
    valueCls?: string
    rows: { left: string; right: string; danger?: boolean }[]
  }[] = [
    {
      label: '节点',
      value: `${online}/${total}`,
      sub: offline > 0 ? `${offline} 台离线` : '全部在线',
      valueCls: offline > 0 ? 'text-amber-500' : 'text-emerald-500',
      // 离线在前，一眼看全谁掉线
      rows: [...servers]
        .sort((a, b) => Number(a.online) - Number(b.online))
        .map((s) => ({
          left: s.name,
          right: s.online ? '在线' : '离线',
          danger: !s.online,
        })),
    },
    {
      label: '月度预算',
      value: budget ? `≈${sym}${budget.monthly.toFixed(2)}` : '—',
      sub: budget ? `${billed} 台计费` : '暂无计费信息',
      rows: [...servers]
        .filter((s) => s.price_per_cycle != null && s.pay_cycle_months && s.pay_cycle_months > 0)
        .sort((a, b) => b.price_per_cycle! / b.pay_cycle_months! - a.price_per_cycle! / a.pay_cycle_months!)
        .map((s) => ({
          left: s.name,
          right: `${CURRENCY_SYMBOL[s.currency ?? 'CNY'] ?? ''}${r2(s.price_per_cycle! / s.pay_cycle_months!)}/月`,
        })),
    },
    {
      label: '剩余价值',
      value: remain && remain.value > 0 ? `≈${CURRENCY_SYMBOL[remain.currency] ?? remain.currency}${remain.value.toFixed(2)}` : '—',
      sub: remain && remain.value > 0 ? `${unexpired} 台未到期` : '暂无到期机器',
      rows: [...servers]
        .filter((s) => s.price_per_cycle != null && s.pay_cycle_months && s.pay_cycle_months > 0 && s.expires_at && s.expires_at > nowSec)
        .sort((a, b) => b.price_per_cycle! / b.pay_cycle_months! * (b.expires_at! - nowSec) - a.price_per_cycle! / a.pay_cycle_months! * (a.expires_at! - nowSec))
        .map((s) => {
          const monthly = s.price_per_cycle! / s.pay_cycle_months!
          const days = Math.max(1, Math.round((s.expires_at! - nowSec) / 86400))
          return {
            left: s.name,
            right: `≈${CURRENCY_SYMBOL[s.currency ?? 'CNY'] ?? ''}${r2((monthly * days) / 30.44)}（${days} 天）`,
          }
        }),
    },
    {
      label: '最忙节点',
      value: busiest && busiest.metrics ? `${busiest.metrics.cpu_usage.toFixed(1)}%` : '—',
      sub: busiest ? busiest.name : '暂无在线节点',
      rows: servers
        .filter((s) => s.online && s.metrics)
        .sort((a, b) => b.metrics!.cpu_usage - a.metrics!.cpu_usage)
        .slice(0, 10)
        .map((s) => ({ left: s.name, right: `${s.metrics!.cpu_usage.toFixed(1)}%` })),
    },
    {
      label: '今日流量',
      value: fmtBytes(today?.in ?? 0),
      sub: `↑ ${fmtBytes(today?.out ?? 0)}`,
      rows: (today?.per_server ?? [])
        .filter((r) => r.in + r.out > 0)
        .map((r) => ({ left: r.name, right: `↓${fmtBytes(r.in)} ↑${fmtBytes(r.out)}` })),
    },
    {
      label: '总流量',
      value: fmtBytes(totalIn),
      sub: `↑ ${fmtBytes(totalOut)}`,
      rows: [...servers]
        .filter((s) => s.metrics)
        .sort((a, b) => b.metrics!.net_in_total + b.metrics!.net_out_total - a.metrics!.net_in_total - a.metrics!.net_out_total)
        .slice(0, 10)
        .map((s) => ({
          left: s.name,
          right: `↓${fmtBytes(s.metrics!.net_in_total)} ↑${fmtBytes(s.metrics!.net_out_total)}`,
        })),
    },
    {
      label: '实时网速',
      value: `↓ ${fmtSpeed(speedIn)}`,
      sub: `↑ ${fmtSpeed(speedOut)}`,
      rows: [...servers]
        .filter((s) => s.metrics)
        .sort((a, b) => b.metrics!.net_in_speed + b.metrics!.net_out_speed - a.metrics!.net_in_speed - a.metrics!.net_out_speed)
        .slice(0, 10)
        .map((s) => ({
          left: s.name,
          right: `↓${fmtSpeed(s.metrics!.net_in_speed)} ↑${fmtSpeed(s.metrics!.net_out_speed)}`,
        })),
    },
  ]

  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-4 xl:grid-cols-7">
      {items.map((it) => (
        <StatCard key={it.label} label={it.label} value={it.value} sub={it.sub} valueCls={it.valueCls} rows={it.rows} />
      ))}
    </div>
  )
}

/** 单张概览卡：悬停弹出明细（有明细行才可弹） */
function StatCard({
  label, value, sub, valueCls, rows,
}: {
  label: string
  value: string
  sub: string
  valueCls?: string
  rows: { left: string; right: string; danger?: boolean }[]
}) {
  const [open, setOpen] = useState(false)
  const hasRows = rows.length > 0
  return (
    <div
      className="relative"
      onMouseEnter={() => hasRows && setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      <Card className={cn('py-3', hasRows && 'cursor-default')}>
        <CardContent className="space-y-1 px-4">
          <div className="text-xs text-muted-foreground">{label}</div>
          <div className={cn('truncate text-lg font-semibold tabular-nums', valueCls)} title={value}>
            {value}
          </div>
          <div className="truncate text-xs text-muted-foreground" title={sub}>
            {sub}
          </div>
        </CardContent>
      </Card>
      {open && hasRows && (
        <div className="absolute left-0 top-full z-50 w-64 max-w-[80vw] pt-1.5">
          <div className="max-h-60 overflow-y-auto rounded-lg border bg-popover p-1.5 text-popover-foreground shadow-lg">
            <div className="px-2 pb-1 pt-0.5 text-xs font-medium text-muted-foreground">{label} · 明细</div>
            {rows.map((r, i) => (
              <div key={i} className="flex items-center justify-between gap-3 rounded-md px-2 py-1.5 text-xs">
                <span className="min-w-0 truncate">{r.left}</span>
                <span className={cn('shrink-0 tabular-nums', r.danger ? 'text-red-500' : 'text-muted-foreground')}>{r.right}</span>
              </div>
            ))}
          </div>
        </div>
      )}
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
  // 极简细线：详细=堆叠大卡 / 紧凑=密横线 / 简约=网格
  const icon =
    value === 'compact' ? <AlignJustify className="size-[18px]" strokeWidth={1.5} /> :
    value === 'mini' ? <Grid2x2 className="size-[18px]" strokeWidth={1.5} /> :
    <GalleryVerticalEnd className="size-[18px]" strokeWidth={1.5} />
  return (
    <DropdownMenu>
      <DropdownMenuTrigger title="卡片形态" className="inline-flex size-8 items-center justify-center rounded-full text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:bg-accent data-[state=open]:bg-accent data-[state=open]:text-foreground">
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
