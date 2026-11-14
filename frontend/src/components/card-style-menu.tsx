// 顶栏「卡片形态」菜单：shadcn DropdownMenu + RadioGroup（当前档打点 + 每档小预览说明）
import { Button } from '@/components/ui/button'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { AlignJustify, Grid2x2, LayoutGrid, Table } from 'lucide-react'
import type { ViewMode } from '@/components/dashboard-header'

const STYLES: { value: ViewMode; label: string; desc: string; Icon: typeof Grid2x2 }[] = [
  { value: 'detailed', label: '详细', desc: '量表 + 速率/到期读数', Icon: AlignJustify },
  { value: 'compact', label: '紧凑', desc: '一行一台的表格', Icon: Table },
  { value: 'mini', label: '简约', desc: '名称 + 状态 + 网速', Icon: Grid2x2 },
]

export function CardStyleMenu({ value, onPick }: {
  value: ViewMode
  onPick: (v: ViewMode) => void
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label="卡片形态" title="卡片形态">
          <LayoutGrid />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuLabel className="text-xs text-muted-foreground">卡片形态</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuRadioGroup value={value} onValueChange={(v) => onPick(v as ViewMode)}>
          {STYLES.map(({ value: v, label, desc, Icon }) => (
            <DropdownMenuRadioItem key={v} value={v} className="gap-2.5 py-2">
              <Icon className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0">
                <span className="block text-sm leading-tight">{label}</span>
                <span className="block truncate text-xs leading-tight text-muted-foreground">{desc}</span>
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
