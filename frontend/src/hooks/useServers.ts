// 仪表盘实时数据：WS 快照 + 增量合并，断线指数退避重连
import { useEffect, useState } from 'react'
import type { DashboardEvent, ServerView } from '@/lib/types'

export type WsStatus = 'connecting' | 'open' | 'closed'

export function useServers() {
  const [servers, setServers] = useState<Map<string, ServerView>>(new Map())
  const [status, setStatus] = useState<WsStatus>('connecting')

  useEffect(() => {
    let ws: WebSocket | null = null
    let retry = 0
    let closed = false
    let timer: ReturnType<typeof setTimeout> | null = null

    const connect = () => {
      if (closed) return
      const proto = location.protocol === 'https:' ? 'wss' : 'ws'
      ws = new WebSocket(`${proto}://${location.host}/ws/dashboard`)
      setStatus('connecting')

      ws.onopen = () => {
        retry = 0
        setStatus('open')
      }
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data as string) as DashboardEvent
          setServers((prev) => {
            const next = new Map(prev)
            switch (msg.type) {
              case 'snapshot':
                next.clear()
                for (const s of msg.servers) next.set(s.id, s)
                break
              case 'update':
                next.set(msg.server.id, msg.server)
                break
              case 'offline': {
                const s = next.get(msg.id)
                if (s) next.set(msg.id, { ...s, online: false })
                break
              }
            }
            return next
          })
        } catch {
          // 忽略无法解析的消息
        }
      }
      ws.onclose = () => {
        setStatus('closed')
        if (!closed) {
          const delay = Math.min(1000 * 2 ** retry, 15000)
          retry++
          timer = setTimeout(connect, delay)
        }
      }
    }

    connect()
    return () => {
      closed = true
      if (timer) clearTimeout(timer)
      ws?.close()
    }
  }, [])

  return { servers, status }
}
