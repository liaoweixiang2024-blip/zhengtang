// 仪表盘实时数据：WS 快照 + 增量合并，断线指数退避重连
import { useEffect, useState } from 'react'
import type { DashboardEvent, ServerView } from '@/lib/types'

export type WsStatus = 'connecting' | 'open' | 'closed'

export function useServers() {
  const [servers, setServers] = useState<Map<string, ServerView>>(new Map())
  const [status, setStatus] = useState<WsStatus>('connecting')
  // 首帧（HTTP 预取或 WS snapshot，先到者）之前不算"看过了列表"——
  // 刷新瞬间绝不能把空列表当"还没有服务器"渲染出来（WS open ≠ 快照已到）
  const [ready, setReady] = useState(false)

  useEffect(() => {
    let ws: WebSocket | null = null
    let retry = 0
    let closed = false
    let timer: ReturnType<typeof setTimeout> | null = null

    // HTTP 预取：挂载即拉一次快照（与 WS 建连并行）。刷新页面时数据几十毫秒就到，
    // 不用干等 WS 握手；WS 连上后的 snapshot 会全量覆盖（clear+set），幂等。
    fetch('/api/servers')
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => {
        if (closed || !d?.servers) return
        setServers((prev) => {
          if (prev.size > 0) return prev // WS 已先到，不回退
          const next = new Map(prev)
          for (const s of d.servers as ServerView[]) next.set(s.id, s)
          return next
        })
        setReady(true)
      })
      .catch(() => {})

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
                setReady(true)
                break
              case 'update':
                next.set(msg.server.id, msg.server)
                break
              case 'offline': {
                const s = next.get(msg.id)
                if (s) next.set(msg.id, { ...s, online: false })
                break
              }
              case 'resort': {
                // 后台拖拽调序：按新 id 顺序重建 Map（Map 迭代序=插入序，
                // 逐台 set 到已有的 key 不会变位置，必须 clear 重插）
                const byId = new Map(next)
                next.clear()
                for (const id of msg.ids) {
                  const s = byId.get(id)
                  if (s) next.set(id, s)
                }
                // 不在列表里的（新机器并发上线）补到尾部
                for (const [id, s] of byId) if (!next.has(id)) next.set(id, s)
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
          // 连续失败多次：校验登录态，session 失效则整页重载走登录流（避免 401 死循环式重连）
          if (retry > 5) {
            fetch('/api/auth/me').then((r) => {
              if (!r.ok) location.reload()
            }).catch(() => {})
          }
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

  return { servers, status, ready }
}
