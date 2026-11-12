// 服务器详情页：系统信息 + CPU/内存/网络/负载历史图表 + GPU
import { useEffect, useMemo, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from '@/components/ui/chart'
import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from 'recharts'
import { fmtBytes } from '@/lib/format'
import type { ServerView } from '@/lib/types'
import { ArrowLeft, Activity, ListTree } from 'lucide-react'

interface DialRow {
  task_id: number
  kind: string
  target: string
  ok: boolean | null
  latency_ms: number | null
  status: string | null
  ts: number | null
}

interface HistoryPoint {
  ts: number
  cpu: number
  mem_used: number
  net_in: number
  net_out: number
  load1: number
}

const RANGES = [
  { key: '1h', label: '1 时' },
  { key: '6h', label: '6 时' },
  { key: '24h', label: '24 时' },
  { key: '7d', label: '7 天' },
  { key: '30d', label: '30 天' },
] as const

export function ServerDetail({ server, onBack }: { server: ServerView; onBack: () => void }) {
  const [range, setRange] = useState<string>('1h')
  const [points, setPoints] = useState<HistoryPoint[]>([])
  const [monthTraffic, setMonthTraffic] = useState(0)
  const [loading, setLoading] = useState(true)
  const [dial, setDial] = useState<DialRow[]>([])

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    fetch(`/api/servers/${server.id}/history?range=${range}`)
      .then((r) => r.json())
      .then((d) => {
        if (cancelled) return
        setPoints(d.points ?? [])
        setMonthTraffic(d.month_traffic ?? 0)
        setLoading(false)
      })
      .catch(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [server.id, range])

  // 拨测结果（每任务最近一条 + 简史）
  useEffect(() => {
    let cancelled = false
    fetch(`/api/servers/${server.id}/dial-results`)
      .then((r) => r.json())
      .then((d) => !cancelled && setDial(d.results ?? []))
    return () => {
      cancelled = true
    }
  }, [server.id, range])

  const chartData = useMemo(
    () =>
      points.map((p) => ({
        ...p,
        time: new Date(p.ts * 1000).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }),
      })),
    [points],
  )

  const si = server.system_info
  const m = server.metrics

  return (
    <div className="space-y-5">
      {/* 顶栏 */}
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="icon" onClick={onBack}>
          <ArrowLeft className="size-4" />
        </Button>
        <h1 className="text-xl font-semibold">{server.name}</h1>
        <span
          className={cn(
            'inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs',
            server.online ? 'bg-emerald-500/10 text-emerald-600' : 'bg-muted text-muted-foreground',
          )}
        >
          <span className={cn('size-1.5 rounded-full', server.online ? 'bg-emerald-500' : 'bg-muted-foreground/40')} />
          {server.online ? '在线' : '离线'}
        </span>
        <div className="ml-auto flex gap-1">
          {RANGES.map((r) => (
            <button
              key={r.key}
              onClick={() => setRange(r.key)}
              className={cn(
                'rounded-md px-2.5 py-1 text-xs transition-colors',
                range === r.key ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-accent',
              )}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>

      {/* 系统信息 */}
      {si && (
        <Card className="py-4">
          <CardContent className="grid grid-cols-2 gap-4 px-4 text-sm sm:grid-cols-4">
            <InfoItem label="系统" value={si.os} />
            <InfoItem label="内核" value={si.kernel} />
            <InfoItem label="架构" value={si.arch} />
            <InfoItem label="虚拟化" value={si.virt} />
            <InfoItem label="CPU" value={`${si.cpu_model} (${si.cpu_cores} 核)`} />
            <InfoItem label="内存" value={fmtBytes(si.mem_total)} />
            <InfoItem label="磁盘" value={fmtBytes(si.disk_total)} />
            <InfoItem label="本月流量" value={fmtBytes(monthTraffic)} />
          </CardContent>
        </Card>
      )}

      {/* GPU 实时 */}
      {m && m.gpus.length > 0 && (
        <Card className="py-4">
          <CardHeader className="pb-0"><CardTitle className="text-sm">GPU</CardTitle></CardHeader>
          <CardContent className="grid gap-4 px-4 pt-2 sm:grid-cols-2 lg:grid-cols-3">
            {m.gpus.map((g, i) => (
              <div key={i} className="rounded-md border p-3 text-sm">
                <div className="mb-2 font-medium">{g.name}</div>
                <div className="flex justify-between text-muted-foreground">
                  <span>利用率</span><span className="tabular-nums">{g.utilization.toFixed(0)}%</span>
                </div>
                {g.mem_total > 0 && (
                  <div className="flex justify-between text-muted-foreground">
                    <span>显存</span>
                    <span className="tabular-nums">{fmtBytes(g.mem_used)} / {fmtBytes(g.mem_total)}</span>
                  </div>
                )}
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      {/* 拨测 */}
      {dial.some((d) => d.ts != null) && (
        <Card className="py-4">
          <CardHeader className="pb-0">
            <CardTitle className="flex items-center gap-2 text-sm font-medium">
              <Activity className="size-4" /> 拨测
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-1.5 px-4 pt-2 text-sm">
            {[...new Map(dial.filter((d) => d.ts != null).map((d) => [d.task_id, d])).values()].map((latest) => {
              const history = dial.filter((d) => d.task_id === latest.task_id)
              const fails = history.filter((d) => d.ok === false).length
              return (
                <div key={latest.task_id} className="flex flex-wrap items-center gap-2 border-b py-1.5 last:border-0">
                  <Badge variant={latest.ok ? 'secondary' : 'destructive'} className="font-mono text-xs">
                    {latest.kind.toUpperCase()}
                  </Badge>
                  <span className="min-w-0 flex-1 truncate font-mono text-xs">{latest.target}</span>
                  {latest.ok ? (
                    <span className="tabular-nums text-emerald-600">{latest.latency_ms}ms</span>
                  ) : (
                    <span className="text-red-500">{latest.status}</span>
                  )}
                  <span className="text-xs text-muted-foreground">
                    最近 20 次 · 失败 {fails}
                  </span>
                </div>
              )
            })}
          </CardContent>
        </Card>
      )}

      {/* 进程监控 */}
      {m && m.processes.length > 0 && (
        <Card className="py-4">
          <CardHeader className="pb-0">
            <CardTitle className="flex items-center gap-2 text-sm font-medium">
              <ListTree className="size-4" /> 进程（共 {m.process_count}）
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-1.5 px-4 pt-2 text-sm">
            {m.processes.map((p) => {
              return (
                <div key={p.rule_id} className="flex items-center gap-3 border-b py-1.5 last:border-0">
                  <span className="font-mono text-xs text-muted-foreground">#{p.rule_id}</span>
                  <Badge variant={p.count > 0 ? 'secondary' : 'destructive'} className="font-mono text-xs">
                    {p.count} 实例
                  </Badge>
                  <span className="tabular-nums">{p.cpu.toFixed(1)}% CPU</span>
                  <span className="tabular-nums text-muted-foreground">{fmtBytes(p.mem_bytes)}</span>
                </div>
              )
            })}
          </CardContent>
        </Card>
      )}

      {/* 图表 */}
      {loading ? (
        <div className="py-20 text-center text-muted-foreground">加载历史数据…</div>
      ) : chartData.length === 0 ? (
        <div className="py-20 text-center text-muted-foreground">
          暂无历史数据（聚合每分钟写入，稍等片刻）
        </div>
      ) : (
        <div className="grid gap-4 lg:grid-cols-2">
          <ChartCard
            title="CPU"
            data={chartData}
            config={{ cpu: { label: 'CPU', color: 'var(--chart-1)' } }}
            yFormatter={(v) => `${v.toFixed(0)}%`}
            domain={[0, 100]}
          />
          <ChartCard
            title="内存"
            data={chartData}
            config={{ mem_used: { label: '内存', color: 'var(--chart-2)' } }}
            yFormatter={(v) => fmtBytes(v, 0)}
            domain={si?.mem_total ? [0, si.mem_total] : undefined}
          />
          <ChartCard
            title="网络（区间流量）"
            data={chartData}
            config={{
              net_in: { label: '下载', color: 'var(--chart-3)' },
              net_out: { label: '上传', color: 'var(--chart-4)' },
            }}
            yFormatter={(v) => fmtBytes(v, 0)}
          />
          <ChartCard
            title="负载（1min）"
            data={chartData}
            config={{ load1: { label: 'load1', color: 'var(--chart-5)' } }}
            yFormatter={(v) => v.toFixed(2)}
          />
        </div>
      )}
    </div>
  )
}

function InfoItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="truncate font-medium" title={value}>{value}</div>
    </div>
  )
}

/** 面积图卡片：config 的 key 即 dataKey（shadcn chart CSS 变量约定 var(--color-key)） */
function ChartCard(props: {
  title: string
  data: Record<string, any>[]
  config: ChartConfig
  yFormatter?: (v: number) => string
  domain?: [number | 'auto', number | 'auto']
}) {
  const { title, data, config, yFormatter, domain } = props
  const dataKeys = Object.keys(config)
  return (
    <Card className="py-4">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
      </CardHeader>
      <CardContent className="px-2">
        <ChartContainer config={config} className="h-48 w-full">
          <AreaChart data={data} margin={{ left: 8, right: 8 }}>
            <defs>
              {dataKeys.map((k) => (
                <linearGradient key={k} id={`fill-${k}`} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="5%" stopColor={`var(--color-${k})`} stopOpacity={0.25} />
                  <stop offset="95%" stopColor={`var(--color-${k})`} stopOpacity={0.03} />
                </linearGradient>
              ))}
            </defs>
            <CartesianGrid vertical={false} strokeDasharray="3 3" />
            <XAxis dataKey="time" tickLine={false} axisLine={false} fontSize={11} interval="preserveStartEnd" />
            <YAxis
              tickLine={false} axisLine={false} fontSize={11} width={48}
              domain={domain ?? ['auto', 'auto']}
              tickFormatter={yFormatter}
            />
            <ChartTooltip content={<ChartTooltipContent indicator="line" />} />
            {dataKeys.map((k) => (
              <Area
                key={k}
                dataKey={k}
                type="monotone"
                stroke={`var(--color-${k})`}
                fill={`url(#fill-${k})`}
                strokeWidth={1.5}
                dot={false}
                isAnimationActive={false}
              />
            ))}
          </AreaChart>
        </ChartContainer>
      </CardContent>
    </Card>
  )
}
