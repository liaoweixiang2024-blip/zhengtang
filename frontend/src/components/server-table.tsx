// 紧凑形态：一行一台表格
import { Badge } from '@/components/ui/badge'
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from '@/components/ui/table'
import { fmtExpires, fmtPercent, fmtSpeed, fmtUptime } from '@/lib/format'
import type { ServerView } from '@/lib/types'
import { cn } from '@/lib/utils'

export function ServerTable({ servers, onOpen }: { servers: ServerView[]; onOpen?: (id: string) => void }) {
  return (
    <div className="rounded-lg border border-border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-8"></TableHead>
            <TableHead>名称</TableHead>
            {/* 手机只留：名称 + 网速（↓↑ 合并一格）+ 到期——全部可见，不用横向划 */}
            <TableHead className="text-right sm:hidden">网速</TableHead>
            <TableHead className="hidden text-right sm:table-cell">CPU</TableHead>
            <TableHead className="hidden text-right sm:table-cell">内存</TableHead>
            <TableHead className="text-right hidden sm:table-cell">下载</TableHead>
            <TableHead className="text-right hidden sm:table-cell">上传</TableHead>
            <TableHead className="text-right hidden md:table-cell">在线时长</TableHead>
            <TableHead className="text-right">到期</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {servers.map((s) => {
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
                <TableCell className="min-w-0 max-w-[40vw] truncate font-medium sm:max-w-none">{s.name}</TableCell>
                <TableCell className="whitespace-nowrap text-right text-[11px] tabular-nums text-muted-foreground sm:hidden">
                  {m ? (
                    <>↓{fmtSpeed(m.net_in_speed)} ↑{fmtSpeed(m.net_out_speed)}</>
                  ) : (
                    '—'
                  )}
                </TableCell>
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
