// 格式化工具（中文单位）

export function fmtBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
  let v = bytes
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(i <= 1 ? 0 : digits)} ${units[i]}`
}

export function fmtSpeed(bytesPerSec: number): string {
  return `${fmtBytes(bytesPerSec)}/s`
}

export function fmtUptime(secs: number): string {
  if (secs <= 0) return '—'
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  if (d > 0) return `${d} 天 ${h} 时`
  if (h > 0) return `${h} 时 ${m} 分`
  return `${m} 分`
}

export function fmtPercent(v: number, digits = 1): string {
  return `${v.toFixed(digits)}%`
}

/** 到期剩余描述：已过期 / N 天 / N 月 / N 年 */
export function fmtExpires(expiresAt: number | null): { text: string; days: number | null } {
  if (!expiresAt) return { text: '长期', days: null }
  const days = Math.floor((expiresAt * 1000 - Date.now()) / 86400000)
  if (days < 0) return { text: `已过期 ${-days} 天`, days }
  if (days === 0) return { text: '今天到期', days }
  if (days < 60) return { text: `${days} 天`, days }
  if (days < 730) return { text: `${(days / 30).toFixed(0)} 个月`, days }
  return { text: `${(days / 365).toFixed(1)} 年`, days }
}

/** 用量颜色：绿 → 黄 → 红 */
export function usageColor(percent: number): string {
  if (percent >= 90) return 'text-red-500'
  if (percent >= 70) return 'text-amber-500'
  return 'text-emerald-500'
}
