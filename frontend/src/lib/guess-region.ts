// 从机器名猜地区（jikasei 同款思路：名字里的城市线索 → 最近国家 → 旗子+城市名）。
// 用于编辑对话框的默认值预填——用户保存即写入，不改 Guess 结果仍可手改。
import { CITY_HINTS, COUNTRY_LL } from './world'

export function guessRegion(name: string): string | null {
  const hint = CITY_HINTS.find((h) => {
    try {
      return h.match.test(name)
    } catch {
      return false
    }
  })
  if (!hint) return null
  // 落点最近的国家码（城市线索表不带国家码，用经纬度就近归国）
  let best = ''
  let bestD = Infinity
  for (const [code, [lon, lat]] of Object.entries(COUNTRY_LL)) {
    const d = (lon - hint.ll[0]) ** 2 + (lat - hint.ll[1]) ** 2
    if (d < bestD) {
      bestD = d
      best = code
    }
  }
  if (!best) return null
  const flag = String.fromCodePoint(...[...best].map((c) => 0x1f1e6 + c.charCodeAt(0) - 65))
  return `${flag} ${hint.name}`
}
