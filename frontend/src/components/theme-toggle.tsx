// 明暗切换（jikasei 同款：单击在亮/暗之间切换，偏好存 localStorage）
import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Moon, Sun } from 'lucide-react'

const KEY = 'zt-theme'

export function ThemeToggle() {
  const [dark, setDark] = useState(() => {
    const saved = localStorage.getItem(KEY)
    if (saved === 'dark') return true
    if (saved === 'light') return false
    return matchMedia('(prefers-color-scheme: dark)').matches
  })

  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
    localStorage.setItem(KEY, dark ? 'dark' : 'light')
  }, [dark])

  return (
    <Button
      variant="ghost"
      size="icon"
      title="切换主题"
      aria-label="切换主题"
      onClick={() => setDark((v) => !v)}
    >
      {dark ? <Sun /> : <Moon />}
    </Button>
  )
}
