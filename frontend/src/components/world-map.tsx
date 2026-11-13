// 点亮世界地图（等距圆柱投影）：有机器的国家整块点亮，可点击筛选；右侧地区列表。
// 数据：world-atlas (Natural Earth PD) topojson + i18n-iso-countries 数字码→ISO2。
import { useMemo, useState } from 'react'
import world from 'world-atlas/countries-50m.json'
import { feature } from 'topojson-client'
import { numericToAlpha2 } from 'i18n-iso-countries'
import { regionOf, regionRows, type GlobeInput } from '@/lib/globe'
import { COUNTRY_NAME } from '@/lib/world'

const W = 960
const H = 480

/** 等距圆柱投影：经纬度 → 画布坐标 */
const px = (lon: number) => ((lon + 180) / 360) * W
const py = (lat: number) => ((90 - lat) / 180) * H

interface CountryPath {
  code: string // ISO alpha2（识别不出的为 ''）
  name: string
  d: string
}

/** topojson → 国家 path（只算一次） */
function buildCountries(): CountryPath[] {
  const fc = feature(world, world.objects.countries) as unknown as {
    features: { id: string; properties: { name: string }; geometry: { type: string; coordinates: unknown } }[]
  }
  const out: CountryPath[] = []
  for (const f of fc.features) {
    if (f.id === '010') continue // 南极不画
    const code = numericToAlpha2(f.id) ?? ''
    const rings = (f.geometry.type === 'Polygon'
      ? f.geometry.coordinates
      : (f.geometry.coordinates as number[][][][]).flat()) as number[][][]
    // 每个环 → 一段子路径
    const d = rings
      .map((ring) =>
        ring
          .map(([lon, lat], i) => `${i === 0 ? 'M' : 'L'}${px(lon).toFixed(1)},${py(lat).toFixed(1)}`)
          .join('') + 'Z',
      )
      .join('')
    if (d) out.push({ code, name: f.properties.name, d })
  }
  return out
}

function RegionFlag({ code }: { code: string }) {
  const [missing, setMissing] = useState(false)
  if (!code || missing) return null
  return (
    <img
      className="globe-reg-flag"
      src={`/flags/${code}.svg`}
      alt=""
      width={16}
      height={12}
      loading="lazy"
      onError={() => setMissing(true)}
    />
  )
}

/** 选中地区的国家码：地区键 "HK · 香港" → HK；直接传码原样 */
function regionToCode(region: string | null): string | null {
  if (!region) return null
  const head = region.split(' · ')[0].trim()
  return /^[A-Z]{2}$/.test(head) ? head : null
}

export function WorldMap({ nodes, region, onRegion }: {
  nodes: GlobeInput[]
  region: string | null
  onRegion: (key: string | null) => void
}) {
  const countries = useMemo(buildCountries, [])
  const rows = useMemo(() => regionRows(nodes), [nodes])
  const selectedCode = regionToCode(region)

  // 国家 → 机器数（点亮）
  const lit = useMemo(() => {
    const map = new Map<string, { count: number; offline: number }>()
    for (const n of nodes) {
      const r = regionOf(n)
      if (!r) continue
      const cur = map.get(r.code) ?? { count: 0, offline: 0 }
      cur.count += 1
      if (!n.online) cur.offline += 1
      map.set(r.code, cur)
    }
    return map
  }, [nodes])

  // 机器落点（地区锚点，在线绿点/离线灰点）
  const dots = useMemo(() => {
    const seen = new Map<string, { x: number; y: number; online: boolean; names: string[] }>()
    for (const n of nodes) {
      const r = regionOf(n)
      if (!r) continue
      const key = r.city ? `${r.code}·${r.city}` : r.code
      const cur = seen.get(key) ?? { x: px(r.base[0]), y: py(r.base[1]), online: true, names: [] }
      cur.online = cur.online && n.online
      cur.names.push(n.name)
      seen.set(key, cur)
    }
    return [...seen.values()]
  }, [nodes])

  const pickCountry = (code: string) => {
    if (!lit.has(code)) return
    onRegion(selectedCode === code ? null : code)
  }

  const total = nodes.length
  const unplaced = total - rows.reduce((s, r) => s + r.count, 0)

  return (
    <div className="globe-panel">
      <div className="globe-atlas" style={{ cursor: 'default' }}>
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMid meet" role="img" aria-label="节点世界地图">
          {countries.map((c) => {
            const hit = lit.get(c.code)
            const selected = selectedCode === c.code
            return (
              <path
                key={c.code || c.name}
                className="map-country"
                d={c.d}
                fill={hit ? 'var(--primary)' : 'var(--muted)'}
                fillOpacity={hit ? (selected ? 0.85 : 0.55) : 0.35}
                stroke="var(--foreground)"
                strokeOpacity={hit ? 0.45 : 0.15}
                strokeWidth={selected ? 1.4 : 0.4}
                style={{ cursor: hit ? 'pointer' : 'default' }}
                onClick={() => pickCountry(c.code)}
              >
                <title>
                  {COUNTRY_NAME[c.code] ?? c.name}
                  {hit ? `：${hit.count} 台${hit.offline ? ` · ${hit.offline} 离线` : ''}` : ''}
                </title>
              </path>
            )
          })}
          {dots.map((d, i) => (
            <circle key={i} cx={d.x} cy={d.y} r={3.2} fill={d.online ? 'var(--primary)' : 'var(--muted-foreground)'} stroke="var(--card)" strokeWidth={1}>
              <title>{d.names.join(' / ')}{d.online ? '' : '（离线）'}</title>
            </circle>
          ))}
        </svg>
      </div>
      <div className="globe-side">
        <div className="globe-side-title">
          地区{total > 0 && unplaced > 0 ? `（${unplaced} 台未标注地区）` : ''}
        </div>
        <div className="globe-rows">
          <button
            type="button"
            className="globe-reg globe-reg-all"
            aria-pressed={region === null}
            onClick={() => onRegion(null)}
          >
            全部<span className="globe-count">{total}</span>
          </button>
          {rows.map((row) => {
            const code = row.region.code
            const active = region === row.region.key || selectedCode === code
            return (
              <button
                key={row.region.key}
                type="button"
                className="globe-reg"
                aria-pressed={active}
                title={row.offline ? `${row.count} 台 · ${row.offline} 台离线` : `${row.count} 台`}
                onClick={() => onRegion(active ? null : row.region.key)}
              >
                <RegionFlag code={code} />
                <span>{row.region.label}</span>
                <b>{row.count}</b>
              </button>
            )
          })}
        </div>
      </div>
    </div>
  )
}
