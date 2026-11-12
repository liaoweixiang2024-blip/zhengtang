// 服务器卡片（详细档对齐 jikasei NodeCard detailed 结构）：
// 旗子+名字 / 在线·价格行 / 2×2 量表 / 三枚读数盒（速率·总量·到期）
import { useState, type ComponentType, type CSSProperties, type ReactNode } from 'react'
import { Card } from '@/components/ui/card'
import { fmtCost } from '@/lib/cost'
import { fmtBytes, fmtSpeed, fmtUptime } from '@/lib/format'
import type { ServerView } from '@/lib/types'
import { cn } from '@/lib/utils'
import { ArrowDown, ArrowDownUp, ArrowUp, CalendarClock, Cpu, HardDrive, MemoryStick } from 'lucide-react'

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

/** 地区 emoji（🇭🇰）→ ISO 国家码（HK），供旗帜文件取图 */
function regionCode(region: string | null): string {
  if (!region) return ''
  const cps = [...region].map((c) => c.codePointAt(0)!)
  const ris = cps.filter((cp) => cp >= 0x1f1e6 && cp <= 0x1f1ff)
  if (ris.length >= 2) {
    return String.fromCodePoint(ris[0] - 0x1f1e6 + 65, ris[1] - 0x1f1e6 + 65)
  }
  return ''
}

/** 旗帜：24×18 圆角方形（jikasei 同款），无国家码/取不到时回退地区原文 */
function Flag({ region }: { region: string | null }) {
  const [missing, setMissing] = useState(false)
  const code = regionCode(region)
  if (!region) return null
  if (!code || missing) {
    return <span className="shrink-0 text-xs text-muted-foreground">{region}</span>
  }
  return (
    <img
      src={`/flags/${code}.svg`}
      alt={code}
      title={region}
      loading="lazy"
      className="h-[18px] w-6 shrink-0 -translate-y-px rounded-[3px] object-cover ring-1 ring-border/70"
      onError={() => setMissing(true)}
    />
  )
}

/** 到期读数：剩余 N 天 / 已过期 N 天 / 今天到期 / 无期限 */
function expiryDays(expiresAt: number | null): number | null {
  if (!expiresAt) return null
  return Math.floor((expiresAt * 1000 - Date.now()) / 86400000)
}

function expiryText(days: number | null): string {
  if (days === null) return '无期限'
  if (days < 0) return `已过期 ${-days} 天`
  if (days === 0) return '今天到期'
  return `剩余 ${days} 天`
}

/** 量表（jikasei Meter 同款：表名+百分比 / 前景色 70% 进度条 / 底注小字） */
function Meter({ label, pct, foot, empty = '—', icon: Icon }: {
  label: ReactNode
  pct: number | null
  foot: ReactNode
  empty?: string
  icon?: ComponentType<{ className?: string; style?: CSSProperties }>
}) {
  const valid = typeof pct === 'number' && Number.isFinite(pct)
  const filled = valid ? Math.min(100, Math.max(0, pct)) : 0
  return (
    <div className="min-w-0">
      <div className="flex items-baseline justify-between gap-2">
        <span className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
          {Icon && <Icon className="size-3 shrink-0" />}
          <span className="truncate">{label}</span>
        </span>
        <span className="tnum text-xs font-medium">
          {valid ? `${filled < 10 ? filled.toFixed(1) : filled.toFixed(0)}%` : empty}
        </span>
      </div>
      <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-muted">
        <div className="h-full rounded-full bg-foreground/70 transition-[width] duration-500" style={{ width: `${filled}%` }} />
      </div>
      <div className="tnum mt-1.5 truncate text-xs text-muted-foreground">{foot}</div>
    </div>
  )
}

/** 读数盒（jikasei InfoBox：浅底小窗） */
function InfoBox({ children, ...rest }: { children: ReactNode } & React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div {...rest} className="min-w-0 space-y-1.5 overflow-hidden rounded-md bg-muted/60 px-2 py-2 text-xs">
      {children}
    </div>
  )
}

function Stat({ icon: Icon, children, wrap }: {
  icon: ComponentType<{ className?: string }>
  children: ReactNode
  wrap?: boolean
}) {
  return (
    <span className="flex min-w-0 items-center gap-1.5">
      <Icon className="size-3 shrink-0" />
      <span className={wrap ? 'tnum truncate stat-wrap' : 'tnum truncate'}>{children}</span>
    </span>
  )
}

/** 详细形态卡：旗子+名字 / 在线·价格 / CPU·内存·硬盘·流量 / 速率·总量·到期 */
export function ServerCard({ server }: { server: ServerView }) {
  const m = server.metrics
  const si = server.system_info
  const cost = fmtCost(server)
  const online = server.online
  const upText = online ? (m ? `在线 ${fmtUptime(m.uptime_secs)}` : '在线') : m ? '离线' : '等待数据…'

  const root = m?.disks.find((d) => d.mount === '/') ?? m?.disks[0]
  const diskPct = root && root.total > 0 ? (root.used / root.total) * 100 : null
  const memPct = m && m.mem_total > 0 ? (m.mem_used / m.mem_total) * 100 : null
  const quota = server.traffic_quota_bytes
  const trafficPct = quota && quota > 0 ? Math.min((server.month_traffic / quota) * 100, 100) : null

  const days = expiryDays(server.expires_at)

  return (
    <Card className={cn('min-w-0 gap-0 p-4 transition-colors hover:ring-foreground/25', !online && 'opacity-60')}>
      {/* 身份行：旗子 + 名字（jikasei：在线与否写在元信息行，不再挂圆点） */}
      <div className="flex min-w-0 items-center gap-2">
        <Flag region={server.region} />
        <h3 className="min-w-0 truncate font-medium">{server.name}</h3>
      </div>

      {/* 元信息行：左 在线时长 / 右 价格·周期 */}
      <div data-meta="" className="mt-2 flex items-center justify-between gap-2 text-xs text-muted-foreground">
        <span className="flex min-w-0 items-center gap-1.5 truncate">
          <OnlineDot online={online} />
          {upText}
        </span>
        {cost && <span className="tnum shrink-0">{cost}</span>}
      </div>

      {/* 无数据/掉线：同一副骨架灰显（与在线卡等高），读数为 — */}
      <>
          {/* 2×2 量表 */}
          <div className="mt-4 grid grid-cols-2 gap-x-4 gap-y-4">
            <Meter
              icon={Cpu}
              label={si ? `CPU ${si.cpu_cores} 核` : 'CPU'}
              pct={m ? m.cpu_usage : null}
              foot={m ? `${m.load1.toFixed(2)} ${m.load5.toFixed(2)} ${m.load15.toFixed(2)}` : '—'}
            />
            <Meter
              icon={MemoryStick}
              label="内存"
              pct={memPct}
              foot={m && m.mem_total > 0 ? `${fmtBytes(m.mem_used)} / ${fmtBytes(m.mem_total)}` : si ? fmtBytes(si.mem_total) : '—'}
            />
            <Meter
              icon={HardDrive}
              label="硬盘"
              pct={diskPct}
              foot={root ? `${fmtBytes(root.used)} / ${fmtBytes(root.total)}` : si ? fmtBytes(si.disk_total) : '—'}
            />
            <Meter
              icon={ArrowDownUp}
              label="流量"
              pct={trafficPct}
              empty="∞"
              foot={quota && quota > 0 ? `${fmtBytes(server.month_traffic)} / ${fmtBytes(quota)}` : `${fmtBytes(server.month_traffic)} / 不限`}
            />
          </div>

          {/* 三枚读数盒：速率 / 总量 / 到期（jikasei [data-stats]，窄卡自动让位） */}
          <div data-stats="" className="mt-4 grid gap-2">
            <InfoBox data-stat="speed">
              <Stat icon={ArrowDown}>{m ? fmtSpeed(m.net_in_speed) : '—'}</Stat>
              <Stat icon={ArrowUp}>{m ? fmtSpeed(m.net_out_speed) : '—'}</Stat>
            </InfoBox>
            <InfoBox data-stat="total">
              <Stat icon={ArrowDown}>{m ? fmtBytes(m.net_in_total) : '—'}</Stat>
              <Stat icon={ArrowUp}>{m ? fmtBytes(m.net_out_total) : '—'}</Stat>
            </InfoBox>
            <InfoBox data-stat="expiry">
              <Stat icon={CalendarClock} wrap>{expiryText(days).replace(/(\d) 天$/, '$1 天')}</Stat>
              {server.expires_at && (
                <span data-stat-date="" className="block truncate pl-[18px] text-muted-foreground">
                  {new Date(server.expires_at * 1000).toISOString().slice(0, 10)}
                </span>
              )}
            </InfoBox>
          </div>
      </>
    </Card>
  )
}

/** 简约形态：名称 + 状态 + 底行两段（CPU/MEM | 网速） */
export function ServerCardMini({ server }: { server: ServerView }) {
  const m = server.metrics
  const memPct = m && m.mem_total > 0 ? (m.mem_used / m.mem_total) * 100 : 0
  return (
    <Card className={cn('min-w-0 gap-0 p-4', !server.online && 'opacity-60')}>
      <div className="flex min-w-0 items-center gap-2">
        <Flag region={server.region} />
        <h3 className="min-w-0 truncate font-medium">{server.name}</h3>
        <span className="ml-auto shrink-0 text-xs text-muted-foreground">
          {server.online ? (m ? fmtUptime(m.uptime_secs) : '在线') : '离线'}
        </span>
      </div>
      {m ? (
        <div className="mt-3 flex items-center justify-between text-xs text-muted-foreground">
          <span>
            CPU <span className="tnum font-medium text-foreground">{m.cpu_usage.toFixed(0)}%</span>
            {'  '}内存 <span className="tnum font-medium text-foreground">{memPct.toFixed(0)}%</span>
          </span>
          <span className="tnum">↓{fmtSpeed(m.net_in_speed)} ↑{fmtSpeed(m.net_out_speed)}</span>
        </div>
      ) : (
        <p className="mt-3 text-xs text-muted-foreground">等待数据…</p>
      )}
    </Card>
  )
}
