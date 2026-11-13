// 第三方数据模块的类型声明（topojson-client 无自带类型；world-atlas 是纯 JSON）
declare module 'topojson-client' {
  export function feature(topology: unknown, object: unknown): { features: unknown[] }
}

declare module 'world-atlas/countries-50m.json' {
  const topology: {
    objects: {
      countries: {
        geometries: { id: string; properties: { name: string } }[]
      }
    }
  }
  export default topology
}
