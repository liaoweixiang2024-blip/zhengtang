import { useEffect, useRef, useState } from 'react'

/**
 * 数值定格：目标值变化后**不滚动**，安静地保持旧值一拍（delay），然后一次性切到新值。
 * （2026-11-15 两轮调优的结论：600ms/1600ms 的缓动滚动都被站长嫌"忙"——
 * 数字翻动的中间过程本身就是噪声，砍掉，只留一次安静的切换。）
 * NaN/Infinity（离线无数据）不参与延迟，直接切。
 */
export function useSmoothNumber(target: number, delay = 1000): number {
  const [display, setDisplay] = useState(target)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    if (display === target || !Number.isFinite(target)) {
      if (!Number.isFinite(target)) setDisplay(target)
      return
    }
    timer.current = setTimeout(() => setDisplay(target), delay)
    return () => {
      if (timer.current) clearTimeout(timer.current)
    }
  }, [target, delay, display])

  return display
}
