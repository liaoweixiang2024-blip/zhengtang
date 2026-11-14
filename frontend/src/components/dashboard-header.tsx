// 概览统计卡片行 + 分组标签 + 形态切换（参考 JIKASEI 主题交互）
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Card } from '@/components/ui/card'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { fmtBytes, fmtSpeed } from '@/lib/format'
import { SmoothValue } from '@/lib/use-smooth'
import { CURRENCY_SYMBOL, costTotalCNY } from '@/lib/cost'
import type { ServerView } from '@/lib/types'
import { cn } from '@/lib/utils'
import { AlignJustify, Check, GalleryVerticalEnd, Grid2x2 } from 'lucide-react'

export type ViewMode = 'detailed' | 'compact' | 'mini'

const VIEW_LABEL: Record<ViewMode, string> = {
  detailed: '详细',
  compact: '紧凑',
  mini: '简约',
}

/** 概览卡（jikasei 版式，4 张卡）：
 *  ① 月度预算 | 剩余价值（两块平分一张卡）
 *  ② 节点 | 最忙节点（同一张卡）
 *  ③ 今日流量（大数=今日 ↓↑，底行=总流量 ↓↑）
 *  ④ 实时网速（大数 ↓↑ + 打开页面至今的走势线）
 *  每块读数悬停弹出每机明细 */
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

  // 月度预算 / 剩余价值：全货币折人民币合计（不再只挑合计最大的一种货币，
  // 美元机与人民币机混编时数字才是真合计）；各币构成进脚注
  const now0 = Date.now() / 1000
  const budget = costTotalCNY(servers, (s) =>
    s.price_per_cycle != null && s.pay_cycle_months && s.pay_cycle_months > 0
      ? s.price_per_cycle / s.pay_cycle_months
      : null)
  const remain = costTotalCNY(servers, (s) => {
    if (s.price_per_cycle == null || !s.pay_cycle_months || s.pay_cycle_months <= 0) return null
    if (!s.expires_at || s.expires_at <= now0) return null
    return ((s.price_per_cycle / s.pay_cycle_months) * (s.expires_at - now0)) / 86400 / 30.44
  })
  // 脚注构成："¥xx + $yy 折合"（单币种时只显示一个）
  const partsText = (parts: { currency: string; amount: number }[]) =>
    parts
      .sort((a, b) => b.amount - a.amount)
      .slice(0, 2)
      .map((p) => `${CURRENCY_SYMBOL[p.currency] ?? p.currency}${p.amount.toFixed(p.amount >= 100 ? 0 : 1)}`)
      .join(' + ') + (parts.length > 2 ? ' …' : '')
  const billed = servers.filter((s) => s.price_per_cycle != null && s.pay_cycle_months && s.pay_cycle_months > 0).length
  const unexpired = servers.filter((s) => s.expires_at && s.expires_at > now0).length

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

  // 速率采样（模块级缓冲：打开页面至今，≥2s 一点，供走势线）
  if (Date.now() - speedSampledAt > 2000) {
    speedSampledAt = Date.now()
    speedHistory.push({ rx: speedIn, tx: speedOut })
    if (speedHistory.length > 240) speedHistory.shift()
  }

  const nowSec = now0
  const r2 = (n: number) => n.toFixed(2)

  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
      {/* ① 月度预算 | 剩余价值 */}
      <Pair>
        <Block
          title="月度预算"
          rows={[...servers]
            .filter((s) => s.price_per_cycle != null && s.pay_cycle_months && s.pay_cycle_months > 0)
            .sort((a, b) => b.price_per_cycle! / b.pay_cycle_months! - a.price_per_cycle! / a.pay_cycle_months!)
            .map((s) => ({
              left: s.name,
              right: `${CURRENCY_SYMBOL[s.currency ?? 'CNY'] ?? ''}${r2(s.price_per_cycle! / s.pay_cycle_months!)}/月`,
            }))}
        >
          <Big>{budget.total > 0 ? `≈¥${budget.total.toFixed(2)}` : '—'}</Big>
          <Foot>
            {budget.parts.length > 0
              ? `${billed} 台计费 · ${budget.parts.length > 1 ? partsText(budget.parts) + ' 折合' : '人民币'}`
              : '暂无计费信息'}
          </Foot>
        </Block>
        <Block
          align="right"
          title="剩余价值"
          rows={[...servers]
            .filter((s) => s.price_per_cycle != null && s.pay_cycle_months && s.pay_cycle_months > 0 && s.expires_at && s.expires_at > nowSec)
            .sort((a, b) => b.price_per_cycle! / b.pay_cycle_months! * (b.expires_at! - nowSec) - a.price_per_cycle! / a.pay_cycle_months! * (a.expires_at! - nowSec))
            .map((s) => {
              const monthly = s.price_per_cycle! / s.pay_cycle_months!
              const days = Math.max(1, Math.round((s.expires_at! - nowSec) / 86400))
              return {
                left: s.name,
                right: `≈${CURRENCY_SYMBOL[s.currency ?? 'CNY'] ?? ''}${r2((monthly * days) / 30.44)}（${days} 天）`,
              }
            })}
        >
          <Big>{remain.total > 0 ? `≈¥${remain.total.toFixed(2)}` : '—'}</Big>
          <Foot>
            {remain.parts.length > 0
              ? `${unexpired} 台未到期${remain.parts.length > 1 ? ` · ${partsText(remain.parts)} 折合` : ''}`
              : '暂无到期机器'}
          </Foot>
        </Block>
      </Pair>

      {/* ② 节点 | 最忙节点 */}
      <Pair>
        <Block
          title="节点"
          rows={[...servers]
            .sort((a, b) => Number(a.online) - Number(b.online))
            .map((s) => ({ left: s.name, right: s.online ? '在线' : '离线', danger: !s.online }))}
        >
          <Big>
            {online} <span className="text-base font-normal text-muted-foreground">/ {total}</span>
          </Big>
          <Foot className={offline > 0 ? 'text-amber-500' : 'text-emerald-500'}>
            {offline > 0 ? `${offline} 台离线` : '全部在线'}
          </Foot>
        </Block>
        <Block
          align="right"
          title="最忙节点"
          rows={servers
            .filter((s) => s.online && s.metrics)
            .sort((a, b) => b.metrics!.cpu_usage - a.metrics!.cpu_usage)
            .slice(0, 10)
            .map((s) => ({ left: s.name, right: `${s.metrics!.cpu_usage.toFixed(1)}%` }))}
        >
          <Big>{busiest && busiest.metrics ? <SmoothValue value={`${busiest.metrics.cpu_usage.toFixed(1)}%`} min="min-w-[5ch]" /> : '—'}</Big>
          <Foot>{busiest ? busiest.name : '暂无在线节点'}</Foot>
        </Block>
      </Pair>

      {/* ③ 今日流量（底行=总流量） */}
      <Tile>
        <Block
          title="今日流量"
          rows={(today?.per_server ?? [])
            .filter((r) => r.in + r.out > 0)
            .map((r) => ({ left: r.name, right: `↓${fmtBytes(r.in)} ↑${fmtBytes(r.out)}` }))}
        >
          <div className="mt-1 grid grid-cols-2 gap-x-2 text-2xl font-semibold tracking-tight tabular-nums">
            <span className="truncate" title={`入站 ${fmtBytes(today?.in ?? 0)}`}>↓ {fmtBytes(today?.in ?? 0)}</span>
            <span className="truncate" title={`出站 ${fmtBytes(today?.out ?? 0)}`}>↑ {fmtBytes(today?.out ?? 0)}</span>
          </div>
          <Foot>
            总流量 <span className="tabular-nums">↓<SmoothValue value={fmtBytes(totalIn)} min="min-w-[7ch]" /> ↑<SmoothValue value={fmtBytes(totalOut)} min="min-w-[7ch]" /></span>
          </Foot>
        </Block>
      </Tile>

      {/* ④ 实时网速 + 走势线 */}
      <Tile>
        <Block
          title="实时网速"
          rows={[...servers]
            .filter((s) => s.metrics)
            .sort((a, b) => b.metrics!.net_in_speed + b.metrics!.net_out_speed - a.metrics!.net_in_speed - a.metrics!.net_out_speed)
            .slice(0, 10)
            .map((s) => ({
              left: s.name,
              right: `↓${fmtSpeed(s.metrics!.net_in_speed)} ↑${fmtSpeed(s.metrics!.net_out_speed)}`,
            }))}
        >
          <div className="mt-1 grid grid-cols-2 gap-x-2 text-2xl font-semibold tracking-tight tabular-nums">
            <span className="truncate">↓ <SmoothValue value={fmtSpeed(speedIn)} /></span>
            <span className="truncate">↑ <SmoothValue value={fmtSpeed(speedOut)} /></span>
          </div>
          <Foot>
            <Sparkline series={speedHistory} />
          </Foot>
        </Block>
      </Tile>
    </div>
  )
}

// 速率采样缓冲（模块级：组件重渲染不清空，走势线记录"打开页面至今"）
const speedHistory: { rx: number; tx: number }[] = []
let speedSampledAt = 0

/** 双读数卡：两块平分（jikasei Pair——整行四张卡读起来是同一个栅格） */
function Pair({ children }: { children: React.ReactNode }) {
  return <Card className="grid min-w-0 grid-cols-2 gap-x-2 gap-y-0 p-4">{children}</Card>
}

/** 单读数卡（jikasei Tile） */
function Tile({ children }: { children: React.ReactNode }) {
  return <Card className="flex min-w-0 flex-col p-4">{children}</Card>
}

/** 单块读数：小标签 / 内容 / 底行小字；rows 非空时悬停弹明细。
 *  明细面板 portal 到 body + fixed 定位：不与下方任何区块（地球/卡片）比层级 */
function Block({
  title, rows, align = 'left', children,
}: {
  title: string
  rows?: { left: string; right: string; danger?: boolean }[]
  align?: 'left' | 'right'
  children: React.ReactNode
}) {
  const [open, setOpen] = useState(false)
  const [spot, setSpot] = useState<{ top: number; left?: number; right?: number; width: number } | null>(null)
  const ref = useRef<HTMLDivElement>(null)
  const hasRows = !!rows?.length
  const enter = () => {
    if (!hasRows) return
    const r = ref.current?.getBoundingClientRect()
    if (r) {
      // 面板宽度跟随触发块（下限 232 保证明细放得下，上限 340），与块边缘严格对齐——
      // 视觉上是"从这块拉出来的抽屉"，而不是飘在页面某处的一个框
      const width = Math.min(340, Math.max(232, Math.ceil(r.width)))
      const top = r.bottom
      // top 贴住块底（零间隙）：容器把视觉间距吃进自身 padding，
      // 鼠标从块移进面板全程都在悬停区内，不会中途触发关闭
      if (align === 'right') {
        setSpot({ top, right: Math.max(8, window.innerWidth - r.right), width })
      } else {
        // 左对齐块左缘；贴近视口右缘时向左让，别溢出屏幕
        setSpot({ top, left: Math.min(r.left, window.innerWidth - width - 8), width })
      }
    }
    setOpen(true)
  }
  return (
    <div
      ref={ref}
      className="relative mt-0 flex min-w-0 flex-1 flex-col"
      onMouseEnter={enter}
      onMouseLeave={() => setOpen(false)}
    >
      <div className="text-xs text-muted-foreground">{title}</div>
      {children}
      {open &&
        hasRows &&
        spot &&
        createPortal(
          <div
            style={{ position: 'fixed', top: spot.top, width: spot.width, ...(spot.left != null ? { left: spot.left } : { right: spot.right }) }}
            className="z-[70] pt-1.5"
            onMouseEnter={() => setOpen(true)}
            onMouseLeave={() => setOpen(false)}
          >
            <div className="max-h-64 animate-in fade-in-0 slide-in-from-top-1 overflow-y-auto rounded-xl border border-border/80 bg-popover p-1.5 text-popover-foreground shadow-lg shadow-black/5 duration-150">
              <div className="flex items-baseline justify-between gap-2 px-2 pb-1 pt-1">
                <span className="text-xs font-medium">{title}</span>
                <span className="text-[10px] text-muted-foreground">每机明细 · {rows!.length}</span>
              </div>
              <div className="grid gap-0.5">
                {rows!.map((r, i) => (
                  <div
                    key={i}
                    className="flex items-center justify-between gap-3 rounded-md px-2 py-1.5 text-xs transition-colors hover:bg-accent"
                  >
                    <span className="min-w-0 truncate">{r.left}</span>
                    <span className={cn('shrink-0 tabular-nums', r.danger ? 'font-medium text-red-500' : 'text-muted-foreground')}>{r.right}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>,
          document.body,
        )}
    </div>
  )
}

/** 大读数（24px；“/ 总数”这类分母降档弱化——jikasei 同款） */
function Big({ children }: { children: React.ReactNode }) {
  return <div className="mt-1 truncate text-2xl font-semibold tracking-tight tabular-nums">{children}</div>
}

/** 底行小字（mt-auto：撑满卡高后贴底） */
function Foot({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn('mt-auto truncate pt-1 text-xs text-muted-foreground', className)}>{children}</div>
}

/** 网速走势线：下行前景色 / 上行弱化灰，无坐标轴，只说形状（jikasei 同款） */
function Sparkline({ series }: { series: { rx: number; tx: number }[] }) {
  const ready = series.length > 1
  const max = Math.max(1, ...series.map((s) => Math.max(s.rx, s.tx)))
  const W = 100
  const H = 28
  const line = (pick: (s: { rx: number; tx: number }) => number) =>
    series
      .map((s, i) => `${((i / (series.length - 1)) * W).toFixed(2)},${(H - 1 - (pick(s) / max) * (H - 2)).toFixed(2)}`)
      .join(' ')
  return (
    <div className="h-5">
      {ready && (
        <svg className="h-full w-full" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
          <polyline points={line((s) => s.rx)} fill="none" strokeWidth={1} vectorEffect="non-scaling-stroke" className="stroke-foreground/70" />
          <polyline points={line((s) => s.tx)} fill="none" strokeWidth={1} vectorEffect="non-scaling-stroke" className="stroke-muted-foreground/60" />
        </svg>
      )}
    </div>
  )
}

/** 分组标签行：全部 + 各组（jikasei 风格：朴素小按钮，激活=浅底、未激活=ghost，横向滚动） */
export function GroupTabs({
  groups, active, onChange,
}: {
  groups: { id: number; name: string; count: number }[]
  active: number | 'all'
  onChange: (g: number | 'all') => void
}) {
  const tabs = [{ id: 'all' as const, name: '全部', count: groups.reduce((a, g) => a + g.count, 0) }, ...groups]
  return (
    <div role="group" aria-label="分组" className="-mx-1 flex min-w-0 flex-1 gap-1 overflow-x-auto px-1 pb-1">
      {tabs.map((t) => (
        <button
          key={String(t.id)}
          aria-pressed={active === t.id}
          onClick={() => onChange(t.id)}
          className={cn(
            'inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md px-3 text-sm outline-none transition-colors',
            active === t.id
              ? 'bg-secondary text-secondary-foreground'
              : 'text-muted-foreground hover:bg-accent hover:text-foreground',
          )}
        >
          {t.name}
          <span className="tabular-nums text-muted-foreground">{t.count}</span>
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
