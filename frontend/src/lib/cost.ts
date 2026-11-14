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

/** 折人民币近似汇率（显示口径用，非结算；2026-11 手动维护，量级对即可） */
export const EXCHANGE_TO_CNY: Record<string, number> = {
  CNY: 1, USD: 7.2, EUR: 7.8, HKD: 0.92, JPY: 0.047, GBP: 9.1,
}

/** 任意金额折人民币（未知货币按 1 处理并在构成里可见） */
export function toCNY(amount: number, currency: string | null | undefined): number {
  return amount * (EXCHANGE_TO_CNY[currency ?? 'CNY'] ?? 1)
}

/** 概览口径：全货币折 CNY 合计 + 各货币构成（脚注展示"¥x + $y 折合"） */
export function costTotalCNY(
  servers: ServerView[],
  valueOf: (s: ServerView) => number | null,
): { total: number; parts: { currency: string; amount: number }[] } {
  const map = new Map<string, number>()
  for (const s of servers) {
    const v = valueOf(s)
    if (v == null || v <= 0) continue
    const cur = s.currency ?? 'CNY'
    map.set(cur, (map.get(cur) ?? 0) + v)
  }
  const parts = [...map.entries()].map(([currency, amount]) => ({ currency, amount }))
  return { total: parts.reduce((acc, p) => acc + toCNY(p.amount, p.currency), 0), parts }
}
