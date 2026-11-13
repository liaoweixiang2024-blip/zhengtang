// 全局错误边界：组件抛错显示错误卡片而非整页白屏
import { Component, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'

interface Props {
  children: ReactNode
}

interface State {
  error: Error | null
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error) {
    console.error('[zhengtang] 页面渲染错误:', error)
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex min-h-svh items-center justify-center bg-background p-4">
          <div className="w-full max-w-md space-y-4 rounded-lg border border-border p-6 text-center">
            <h1 className="text-lg font-semibold">页面出错了</h1>
            <p className="break-all font-mono text-xs text-muted-foreground">
              {this.state.error.message}
            </p>
            <div className="flex justify-center gap-2">
              <Button
                size="sm"
                onClick={() => {
                  this.setState({ error: null })
                  location.assign('/')
                }}
              >
                回到首页
              </Button>
              <Button size="sm" variant="outline" onClick={() => location.reload()}>
                刷新页面
              </Button>
            </div>
          </div>
        </div>
      )
    }
    return this.props.children
  }
}
