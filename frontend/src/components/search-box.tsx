// 顶栏搜索框（jikasei 同款交互）：收起时是一枚方形图标，点开就地长成输入框
import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Search, X } from 'lucide-react'

export function SearchBox({ value, onChange }: {
  value: string
  onChange: (v: string) => void
}) {
  const [open, setOpen] = useState(false)
  const input = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open) input.current?.focus()
  }, [open])

  if (!open) {
    return (
      <Button
        variant="ghost"
        size="icon"
        title="搜索"
        aria-label="搜索"
        onClick={() => setOpen(true)}
      >
        <Search />
      </Button>
    )
  }
  return (
    <div className="relative flex h-9 w-40 items-center sm:w-56">
      <Search className="pointer-events-none absolute left-2.5 size-4 text-muted-foreground" />
      <Input
        ref={input}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            onChange('')
            setOpen(false)
          }
        }}
        placeholder="搜索名称…"
        className="h-9 pl-8 pr-8"
      />
      {value && (
        <button
          type="button"
          aria-label="清空"
          className="absolute right-2 inline-flex size-5 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
          onClick={() => onChange('')}
        >
          <X className="size-3.5" />
        </button>
      )}
    </div>
  )
}
