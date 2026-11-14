import { cn } from '@/lib/utils'

/**
 * 数值显示的统一出口：
 * - 零动画直切（2026-11-15 四轮调优结论：滚动/定格/滑入都被否，不加戏最安静）
 * - 等宽数字 + 按格式预留最小宽度：数字宽度恒定，值变化不再推挤周围布局
 *   （单位换挡如 999KB→1.0MB 的字符数变化也被 min-width 兜住）
 * min 传 tailwind 宽度类（如 "min-w-[8ch]"）：网速 8ch、字节量 7ch、百分比 5ch
 */
export function SmoothValue({ value, min }: { value: string; min?: string }) {
  return <span className={cn('inline-block text-right tabular-nums', min)}>{value}</span>
}
