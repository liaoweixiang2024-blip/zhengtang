// 登录页 + 首启向导（setup_needed 时重定向到向导）
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

export function Login({ setupNeeded, onDone }: { setupNeeded: boolean; onDone: () => void }) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [totp, setTotp] = useState('')
  const [totpRequired, setTotpRequired] = useState(false)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      // 首启向导与登录共用表单：无用户时提交到 /api/setup
      const res = await fetch(setupNeeded ? '/api/setup' : '/api/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(setupNeeded ? { username, password } : { username, password, totp: totp || undefined }),
      })
      const d = await res.json()
      if (res.ok) {
        onDone()
      } else if (d.totp_required) {
        setTotpRequired(true)
        setError('请输入两步验证码')
      } else {
        setError(d.error ?? '登录失败')
      }
    } catch {
      setError('网络错误')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex min-h-svh items-center justify-center bg-background p-4">
      <Card className="w-full max-w-sm">
        <CardHeader className="text-center">
          <CardTitle className="text-xl">zhengtang</CardTitle>
          <CardDescription>{setupNeeded ? '创建管理员账号' : '登录探针面板'}</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submit} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">用户名</Label>
              <Input id="username" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" required />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">密码</Label>
              <Input id="password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" required />
            </div>
            {totpRequired && (
              <div className="space-y-2">
                <Label htmlFor="totp">两步验证码</Label>
                <Input id="totp" inputMode="numeric" maxLength={6} value={totp} onChange={(e) => setTotp(e.target.value)} placeholder="6 位验证码" autoFocus />
              </div>
            )}
            {error && <p className="text-sm text-destructive">{error}</p>}
            <Button type="submit" className="w-full" disabled={busy}>
              {busy ? '请稍候…' : setupNeeded ? '创建管理员并登录' : '登录'}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
