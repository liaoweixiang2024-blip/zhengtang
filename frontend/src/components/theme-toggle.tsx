// 明暗切换：随系统 / 亮 / 暗（偏好存 localStorage）
import { useEffect, useState } from 'react'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { Check, Monitor, Moon, Sun } from 'lucide-react'

type Mode = 'system' | 'light' | 'dark'
const KEY = 'zt-theme'

function apply(mode: Mode) {
  const dark = mode === 'dark' || (mode === 'system' && matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.classList.toggle('dark', dark)
}

export function ThemeToggle() {
  const [mode, setMode] = useState<Mode>(() => (localStorage.getItem(KEY) as Mode) || 'system')

  useEffect(() => {
    apply(mode)
    localStorage.setItem(KEY, mode)
    const mq = matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => mode === 'system' && apply('system')
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [mode])

  const icon = mode === 'dark' ? <Moon className="size-[18px]" /> : mode === 'light' ? <Sun className="size-[18px]" /> : <Monitor className="size-[18px]" />
  return (
    <DropdownMenu>
      <DropdownMenuTrigger title="界面主题" className="inline-flex size-8 items-center justify-center rounded-full text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/40 data-[state=open]:bg-accent data-[state=open]:text-foreground">
        {icon}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {([['system', '随系统', <Monitor key="i" className="size-4" />], ['light', '亮色', <Sun key="i" className="size-4" />], ['dark', '暗色', <Moon key="i" className="size-4" />]] as const).map(([m, label, i]) => (
          <DropdownMenuItem key={m} onClick={() => setMode(m)} className="gap-2">
            {i}
            {label}
            <Check className={cn('ml-auto size-4', mode !== m && 'invisible')} />
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
