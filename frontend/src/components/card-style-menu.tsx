// 顶栏「卡片形态」菜单：shadcn DropdownMenu + RadioGroup（当前档打点）
import { Button } from '@/components/ui/button'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { AlignJustify, Grid2x2, LayoutGrid, Table } from 'lucide-react'
import type { ViewMode } from '@/components/dashboard-header'

const STYLES: { value: ViewMode; label: string; Icon: typeof Grid2x2 }[] = [
  { value: 'detailed', label: '详细', Icon: AlignJustify },
  { value: 'compact', label: '紧凑', Icon: Table },
  { value: 'mini', label: '简约', Icon: Grid2x2 },
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
      <DropdownMenuContent align="end" className="w-36">
        <DropdownMenuRadioGroup value={value} onValueChange={(v) => onPick(v as ViewMode)}>
          {STYLES.map(({ value: v, label, Icon }) => (
            <DropdownMenuRadioItem key={v} value={v} className="gap-2">
              <Icon className="size-4 text-muted-foreground" />
              {label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
