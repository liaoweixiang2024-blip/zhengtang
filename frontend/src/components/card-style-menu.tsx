// 顶栏「卡片形态」菜单（jikasei 同款）：ghost 图标按钮 + 浮层菜单（勾选当前档）
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Button } from '@/components/ui/button'
import { AlignJustify, Check, Grid2x2, LayoutGrid, Table } from 'lucide-react'
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
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLSpanElement>(null)
  const menu = useRef<HTMLDivElement>(null)
  // 浮层挂到 header（按钮住在 overflow-x-auto 条带里，直接展开会被裁）
  const [spot, setSpot] = useState<{ host: HTMLElement; top: number; right: number } | null>(null)

  useLayoutEffect(() => {
    if (!open) return
    const place = () => {
      const b = box.current?.getBoundingClientRect()
      const host = box.current?.closest<HTMLElement>('header')
      const h = host?.getBoundingClientRect()
      if (b && host && h) setSpot({ host, top: b.bottom - h.top + 4, right: h.right - b.right })
    }
    place()
    window.addEventListener('resize', place)
    return () => window.removeEventListener('resize', place)
  }, [open])

  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      const el = e.target
      if (!(el instanceof HTMLElement) || !(box.current?.contains(el) || menu.current?.contains(el))) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('pointerdown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  return (
    <span className="relative inline-flex" ref={box}>
      <Button
        variant="ghost"
        size="icon"
        className="card-style-toggle"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="卡片形态"
        title="卡片形态"
        onClick={() => setOpen((v) => !v)}
      >
        <LayoutGrid />
      </Button>
      {open && spot && createPortal(
        <div
          ref={menu}
          role="menu"
          aria-label="卡片形态"
          style={{ top: spot.top, right: spot.right }}
          className="absolute z-20 block w-36 overflow-hidden rounded-md border bg-popover py-1 text-sm shadow-md"
        >
          {STYLES.map(({ value: v, label, Icon }) => (
            <button
              key={v}
              type="button"
              role="menuitemradio"
              aria-checked={v === value}
              className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left hover:bg-accent"
              onClick={() => {
                onPick(v)
                setOpen(false)
              }}
            >
              <Icon className="size-4 shrink-0 text-muted-foreground" />
              <span className="flex-1 truncate">{label}</span>
              {v === value && <Check className="size-3.5 shrink-0" />}
            </button>
          ))}
        </div>,
        spot.host,
      )}
    </span>
  )
}
