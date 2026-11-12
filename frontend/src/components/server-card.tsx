// 服务器卡片：详细形态（默认）与简约形态
import { Card, CardContent } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import {
  fmtBytes, fmtExpires, fmtPercent, fmtSpeed, fmtUptime, usageColor,
} from '@/lib/format'
import { fmtCost } from '@/lib/cost'
import type { ServerView } from '@/lib/types'
import { cn } from '@/lib/utils'

function OnlineDot({ online }: { online: boolean }) {
  return (
    <span
      className={cn(
        'inline-block size-2 rounded-full',
        online ? 'bg-emerald-500 animate-pulse' : 'bg-muted-foreground/40',
      )}
    />
  )
}

/** 到期徽章：文案带"到期"字样 + 悬停显示具体日期；临期渐红 */
function ExpiresBadge({ expiresAt }: { expiresAt: number | null }) {
  const { text, days } = fmtExpires(expiresAt)
  if (days === null) return <Badge variant="outline" className="font-normal">长期</Badge>
  const variant = days < 0 || days <= 7 ? 'destructive' : days <= 30 ? 'default' : 'secondary'
  const date = new Date(expiresAt! * 1000).toISOString().slice(0, 10)
  return (
    <Badge variant={variant} className="font-normal" title={`到期日 ${date}`}>
      {text}
    </Badge>
  )
}

/** 费用短标（有付费信息才显示；免费高亮为绿色） */
function CostTag({ server }: { server: ServerView }) {
  const cost = fmtCost(server)
  if (!cost) return null
  return (
    <span className={cn('text-xs tabular-nums', server.pay_cycle_months === -1 ? 'text-emerald-500' : 'text-muted-foreground')}>
      {cost}
    </span>
  )
}

/** 详细形态：系统信息 + 四格读数 + 流量/磁盘条 + 到期 */
export function ServerCard({ server }: { server: ServerView }) {
  const m = server.metrics
  const si = server.system_info
  const memPct = m && m.mem_total > 0 ? (m.mem_used / m.mem_total) * 100 : 0
  const swapPct = m && m.swap_total > 0 ? (m.swap_used / m.swap_total) * 100 : 0
  const root = m?.disks.find((d) => d.mount === '/') ?? m?.disks[0]
  const diskPct = root && root.total > 0 ? (root.used / root.total) * 100 : 0
  const netTotal = m ? m.net_in_total + m.net_out_total : 0

  return (
    <Card className={cn('gap-3 py-4', !server.online && 'opacity-60')}>
      <CardContent className={cn('px-4', server.online && m ? 'space-y-3' : 'flex flex-1 flex-col items-center justify-center gap-1.5 py-8')}>
        {/* 头部两行：① 状态点+名称 | 到期徽章；② 系统信息 | 费用 */}
        <div className="w-full space-y-1">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <OnlineDot online={server.online} />
              <span className="truncate font-medium">{server.name}</span>
            </div>
            <ExpiresBadge expiresAt={server.expires_at} />
          </div>
          {(server.region || si || fmtCost(server)) && (
            <div className="flex items-center justify-between gap-2 text-xs">
              <span className="flex min-w-0 items-center gap-1.5">
                {server.region && (
                  <span className="shrink-0 rounded-full bg-muted px-1.5 py-0.5 leading-none">{server.region}</span>
                )}
                {si && (
                  <span className="min-w-0 truncate text-muted-foreground">
                    {si.os} · {si.cpu_cores}C {fmtBytes(si.mem_total, 0)}
                  </span>
                )}
              </span>
              <CostTag server={server} />
            </div>
          )}
        </div>

        {server.online && m ? (
          <>
            {/* 四格读数 */}
            <div className="grid grid-cols-4 gap-2 text-center">
              <div>
                <div className={cn('text-sm font-semibold tabular-nums', usageColor(m.cpu_usage))}>
                  {fmtPercent(m.cpu_usage, 0)}
                </div>
                <div className="text-[10px] text-muted-foreground">CPU</div>
              </div>
              <div>
                <div className={cn('text-sm font-semibold tabular-nums', usageColor(memPct))}>
                  {fmtPercent(memPct, 0)}
                </div>
                <div className="text-[10px] text-muted-foreground">内存</div>
              </div>
              <div>
                <div className="text-sm font-semibold tabular-nums text-sky-500">
                  ↓{fmtSpeed(m.net_in_speed)}
                </div>
                <div className="text-[10px] text-muted-foreground">下载</div>
              </div>
              <div>
                <div className="text-sm font-semibold tabular-nums text-violet-500">
                  ↑{fmtSpeed(m.net_out_speed)}
                </div>
                <div className="text-[10px] text-muted-foreground">上传</div>
              </div>
            </div>

            {/* 内存/交换/磁盘进度条 */}
            <div className="space-y-1.5">
              <BarRow label="内存" pct={memPct} detail={`${fmtBytes(m.mem_used)} / ${fmtBytes(m.mem_total)}`} />
              {m.swap_total > 0 && (
                <BarRow label="交换" pct={swapPct} detail={`${fmtBytes(m.swap_used)} / ${fmtBytes(m.swap_total)}`} />
              )}
              {root && (
                <BarRow
                  label="磁盘"
                  pct={diskPct}
                  detail={`${fmtBytes(root.used)} / ${fmtBytes(root.total)}`}
                />
              )}
              {server.traffic_quota_bytes && server.traffic_quota_bytes > 0 && (
                <BarRow
                  label="月流量"
                  pct={Math.min((netTotal / server.traffic_quota_bytes) * 100, 100)}
                  detail={`${fmtBytes(netTotal)} / ${fmtBytes(server.traffic_quota_bytes)}`}
                />
              )}
            </div>

            {/* 底行：在线时长 + 负载 + 总流量 */}
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>{fmtUptime(m.uptime_secs)}</span>
              <span>负载 {m.load1.toFixed(2)}</span>
              <span>累计 {fmtBytes(netTotal)}</span>
            </div>
          </>
        ) : (
          <>
            <span className="text-sm">{m ? '离线' : '等待数据…'}</span>
            {m && <span className="text-xs text-muted-foreground">累计 {fmtBytes(m.net_in_total + m.net_out_total)}</span>}
          </>
        )}
      </CardContent>
    </Card>
  )
}

function BarRow({ label, pct, detail }: { label: string; pct: number; detail: string }) {
  return (
    <div className="flex items-center gap-2 text-xs">
      <span className="w-10 shrink-0 text-muted-foreground">{label}</span>
      <Progress
        value={pct}
        className={cn('h-1.5 flex-1', pct >= 90 && '[&>div]:bg-red-500', pct >= 70 && pct < 90 && '[&>div]:bg-amber-500')}
      />
      <span className="shrink-0 tabular-nums text-muted-foreground">{detail}</span>
    </div>
  )
}

/** 简约形态：名称 + 状态 + 底行两段（CPU/MEM | 网速） */
export function ServerCardMini({ server }: { server: ServerView }) {
  const m = server.metrics
  const memPct = m && m.mem_total > 0 ? (m.mem_used / m.mem_total) * 100 : 0
  return (
    <Card className={cn('gap-2 py-3', !server.online && 'opacity-60')}>
      <CardContent className={cn('px-4', server.online && m ? 'space-y-2' : 'flex flex-1 flex-col items-center justify-center gap-1.5 py-4')}>
        <div className="flex w-full items-center justify-between">
          <div className="flex min-w-0 items-center gap-2">
            <OnlineDot online={server.online} />
            {server.region && <span className="shrink-0 leading-none" title={server.region}>{server.region.split(' ')[0]}</span>}
            <span className="truncate text-sm font-medium">{server.name}</span>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <CostTag server={server} />
            <ExpiresBadge expiresAt={server.expires_at} />
          </div>
        </div>
        {server.online && m ? (
          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span>
              CPU <span className={cn('font-semibold tabular-nums', usageColor(m.cpu_usage))}>{fmtPercent(m.cpu_usage, 0)}</span>
              {'  '}内存 <span className={cn('font-semibold tabular-nums', usageColor(memPct))}>{fmtPercent(memPct, 0)}</span>
            </span>
            <span className="tabular-nums">↓{fmtSpeed(m.net_in_speed)} ↑{fmtSpeed(m.net_out_speed)}</span>
          </div>
        ) : (
          <>
            <span className="text-xs">{m ? '离线' : '等待数据…'}</span>
            {m && <span className="text-[10px] text-muted-foreground">累计 {fmtBytes(m.net_in_total + m.net_out_total)}</span>}
          </>
        )}
      </CardContent>
    </Card>
  )
}
