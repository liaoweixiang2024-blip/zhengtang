// 紧凑形态：一行一台表格
import { useState } from 'react'
import { Badge } from '@/components/ui/badge'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { fmtExpires, fmtPercent, fmtSpeed, fmtUptime } from '@/lib/format'
import type { ServerView } from '@/lib/types'
import { cn } from '@/lib/utils'
import { ArrowDownAZ, ArrowUpAZ, ArrowUpDown } from 'lucide-react'

/** 到期排序：null=面板手动序（默认），asc=最早到期在前，desc=最晚在前。
 *  纯视图层一次性排序（组件内部 state，不落库、切走视图/刷新即恢复手动序）。
 *  无到期日的机器：asc 时排最后（长期机器不参与紧迫排序），desc 时排最前 */
type ExpSort = null | 'asc' | 'desc'

export function ServerTable({ servers, onOpen }: { servers: ServerView[]; onOpen?: (id: string) => void }) {
  const [expSort, setExpSort] = useState<ExpSort>(null)
  const rows = expSort
    ? [...servers].sort((a, b) => {
        const av = a.expires_at ?? (expSort === 'asc' ? Infinity : -1)
        const bv = b.expires_at ?? (expSort === 'asc' ? Infinity : -1)
        return expSort === 'asc' ? av - bv : bv - av
      })
    : servers
  const cycleSort = () => setExpSort((v) => (v === null ? 'asc' : v === 'asc' ? 'desc' : null))
  const SortIcon = expSort === 'asc' ? ArrowDownAZ : expSort === 'desc' ? ArrowUpAZ : ArrowUpDown

  return (
    <div className="rounded-lg border border-border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-8"></TableHead>
            <TableHead>名称</TableHead>
            {/* 手机只留：名称 + 到期（此前含网速合并列仍会溢出，砍掉——看全 > 看多） */}
            <TableHead className="hidden text-right sm:table-cell">CPU</TableHead>
            <TableHead className="hidden text-right sm:table-cell">内存</TableHead>
            <TableHead className="text-right hidden sm:table-cell">下载</TableHead>
            <TableHead className="text-right hidden sm:table-cell">上传</TableHead>
            <TableHead className="text-right hidden md:table-cell">在线时长</TableHead>
            <TableHead className="text-right">
              <button
                type="button"
                onClick={cycleSort}
                title={expSort === null ? '按到期排序（最早在前）' : expSort === 'asc' ? '按到期排序（最晚在前）' : '恢复面板顺序'}
                className={cn(
                  'inline-flex items-center gap-1 rounded px-1 py-0.5 transition-colors hover:text-foreground',
                  expSort && 'text-foreground',
                )}
              >
                到期
                <SortIcon className="size-3.5" />
              </button>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((s) => {
            const m = s.metrics
            const memPct = m && m.mem_total > 0 ? (m.mem_used / m.mem_total) * 100 : 0
            return (
              <TableRow key={s.id} className={cn('cursor-pointer', !s.online && 'opacity-60')} onClick={() => onOpen?.(s.id)}>
                <TableCell>
                  <span
                    className={cn(
                      'inline-block size-2 rounded-full',
                      s.online ? 'bg-emerald-500' : 'bg-muted-foreground/40',
                    )}
                  />
                </TableCell>
                <TableCell className="min-w-0 truncate font-medium">{s.name}</TableCell>
                <TableCell className="hidden text-right tabular-nums sm:table-cell">
                  {m ? fmtPercent(m.cpu_usage, 0) : '—'}
                </TableCell>
                <TableCell className="hidden text-right tabular-nums sm:table-cell">
                  {m ? fmtPercent(memPct, 0) : '—'}
                </TableCell>
                <TableCell className="hidden text-right tabular-nums sm:table-cell">
                  {m?.net_in_speed !== undefined ? fmtSpeed(m.net_in_speed) : '—'}
                </TableCell>
                <TableCell className="hidden text-right tabular-nums sm:table-cell">
                  {m?.net_out_speed !== undefined ? fmtSpeed(m.net_out_speed) : '—'}
                </TableCell>
                <TableCell className="hidden text-right text-muted-foreground md:table-cell">
                  {m ? fmtUptime(m.uptime_secs) : '—'}
                </TableCell>
                <TableCell className="whitespace-nowrap text-right">
                  <ExpiresCell expiresAt={s.expires_at} />
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}

function ExpiresCell({ expiresAt }: { expiresAt: number | null }) {
  const { text, days } = fmtExpires(expiresAt)
  if (days === null) return <span className="text-muted-foreground">长期</span>
  if (days < 0 || days <= 7) return <Badge variant="destructive" className="font-normal">{text}</Badge>
  if (days <= 30) return <Badge className="font-normal">{text}</Badge>
  return <span className="text-muted-foreground">{text}</span>
}
