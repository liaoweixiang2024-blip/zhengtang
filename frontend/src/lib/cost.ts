// 付费周期与费用展示（卡片 / 管理页 / 详情页共用）
import type { ServerView } from '@/lib/types'

export const CYCLE_LABEL: Record<number, string> = { 1: '月付', 3: '季付', 12: '年付', 36: '3年付', 60: '5年付', 0: '一次性', [-1]: '免费' }
export const CURRENCY_SYMBOL: Record<string, string> = { CNY: '¥', USD: '$', EUR: '€', JPY: 'JP¥', HKD: 'HK$', GBP: '£' }

export function fmtCost(s: { price_per_cycle: number | null; pay_cycle_months: number | null; currency: string | null }): string {
  const months = s.pay_cycle_months
  if (months === -1) return '免费'
  if (s.price_per_cycle == null || months == null) return ''
  const sym = CURRENCY_SYMBOL[s.currency ?? 'CNY'] ?? ''
  const price = String(s.price_per_cycle).replace(/\.0+$/, '')
  return `${sym}${price}/${CYCLE_LABEL[months] ?? `${months}个月`}`
}

/** 各货币月均合计（一次性/免费不计入） */
export function monthlyCostByCurrency(servers: ServerView[]): { currency: string; monthly: number }[] {
  const map = new Map<string, number>()
  for (const s of servers) {
    if (s.price_per_cycle == null || !s.pay_cycle_months || s.pay_cycle_months <= 0) continue
    const cur = s.currency ?? 'CNY'
    map.set(cur, (map.get(cur) ?? 0) + s.price_per_cycle / s.pay_cycle_months)
  }
  return [...map.entries()].map(([currency, monthly]) => ({ currency, monthly }))
}

/** 剩余价值：未到期机器按 月均 × 剩余天数/30.44 折算，按货币合计 */
export function remainingValueByCurrency(servers: ServerView[]): { currency: string; value: number }[] {
  const map = new Map<string, number>()
  const now = Date.now() / 1000
  for (const s of servers) {
    if (s.price_per_cycle == null || !s.pay_cycle_months || s.pay_cycle_months <= 0) continue
    if (!s.expires_at || s.expires_at <= now) continue
    const monthly = s.price_per_cycle / s.pay_cycle_months
    const remainDays = (s.expires_at - now) / 86400
    const cur = s.currency ?? 'CNY'
    map.set(cur, (map.get(cur) ?? 0) + (monthly * remainDays) / 30.44)
  }
  return [...map.entries()].map(([currency, value]) => ({ currency, value }))
}
