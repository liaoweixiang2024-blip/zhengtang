import { useEffect, useRef, useState } from 'react'

/**
 * 数值平滑：目标值变化时在 duration 内缓动插值（rAF），数字"滑"过去而不是 3 秒一跳。
 * 站长口径：概览/卡片每 3s 刷新一次，600ms 的 easeOutCubic 正好在两次刷新间
 * 完成过渡、又不拖泥带水。NaN/Infinity（离线无数据）直接跳变不插值。
 */
export function useSmoothNumber(target: number, duration = 600): number {
  const [display, setDisplay] = useState(target)
  const fromRef = useRef(target)
  const rafRef = useRef(0)

  useEffect(() => {
    const from = fromRef.current
    if (from === target || !Number.isFinite(target) || !Number.isFinite(from)) {
      fromRef.current = target
      setDisplay(target)
      return
    }
    const start = performance.now()
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / duration)
      const eased = 1 - Math.pow(1 - t, 3) // easeOutCubic：先快后缓，收尾稳
      const v = from + (target - from) * eased
      fromRef.current = v
      setDisplay(v)
      if (t < 1) rafRef.current = requestAnimationFrame(tick)
    }
    cancelAnimationFrame(rafRef.current)
    rafRef.current = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(rafRef.current)
  }, [target, duration])

  return display
}
