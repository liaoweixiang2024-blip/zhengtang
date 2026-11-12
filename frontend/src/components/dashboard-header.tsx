// 概览统计卡片行 + 分组标签 + 形态切换（参考 JIKASEI 主题交互）
import { Card, CardContent } from '@/components/ui/card'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { fmtBytes } from '@/lib/format'
import type { ServerView } from '@/lib/types'
import { cn } from '@/lib/utils'
import { Check, Grid2x2, LayoutGrid, Rows3 } from 'lucide-react'

export type ViewMode = 'detailed' | 'compact' | 'mini'

const VIEW_LABEL: Record<ViewMode, string> = {
  detailed: '详细',
  compact: '紧凑',
  mini: '简约',
}

/** 概览卡片：总数 / 在线 / 离线 / 累计流量 */
export function StatCards({ servers }: { servers: ServerView[] }) {
  const total = servers.length
  const online = servers.filter((s) => s.online).length
  const offline = total - online
  const netTotal = servers.reduce(
    (acc, s) => acc + (s.metrics ? s.metrics.net_in_total + s.metrics.net_out_total : 0),
    0,
  )
  const items = [
    { label: '服务器', value: String(total) },
    { label: '在线', value: String(online), cls: 'text-emerald-500' },
    { label: '离线', value: String(offline), cls: offline > 0 ? 'text-red-500' : undefined },
    { label: '累计流量', value: fmtBytes(netTotal) },
  ]
  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
      {items.map((it) => (
        <Card key={it.label} className="py-3">
          <CardContent className="px-4">
            <div className={cn('text-xl font-semibold tabular-nums', it.cls)}>{it.value}</div>
            <div className="text-xs text-muted-foreground">{it.label}</div>
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
