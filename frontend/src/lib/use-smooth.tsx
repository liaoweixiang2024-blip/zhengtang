import { cn } from '@/lib/utils'

/**
 * 交易所式数字：数值从不插值（不会出现 11.87 这种中间数），
 * 变化瞬间新值以一次短促的滑入动画顶替旧值——同行情软件的报价切换。
 *
 * 实现：span 以格式化结果为 key，值一变元素即重挂载，CSS 进入动画随之播放。
 * 300ms / 从下 6px 滑入 + 淡入，安静且有"换了一手"的方向感。
 */
export function SmoothValue({ value, className }: { value: string; className?: string }) {
  return (
    <span
      key={value}
      className={cn('inline-block animate-in fade-in-0 slide-in-from-bottom-1.5 duration-300', className)}
    >
      {value}
    </span>
  )
}
