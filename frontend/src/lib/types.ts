// 与后端 proto::RealtimeMetrics / SystemInfo / ServerView 对齐
export interface DiskUsage {
  mount: string
  fs: string
  total: number
  used: number
}

export interface GpuInfo {
  name: string
  utilization: number
  mem_total: number
  mem_used: number
  temperature: number
}

export interface ProcStat {
  rule_id: number
  count: number
  cpu: number
  mem_bytes: number
}

export interface RealtimeMetrics {
  cpu_usage: number
  cpu_per_core: number[]
  mem_total: number
  mem_used: number
  swap_total: number
  swap_used: number
  load1: number
  load5: number
  load15: number
  uptime_secs: number
  net_in_total: number
  net_out_total: number
  net_in_speed: number
  net_out_speed: number
  disks: DiskUsage[]
  gpus: GpuInfo[]
  processes: ProcStat[]
  process_count: number
  tcp_conn_count: number
  udp_conn_count: number
}

export interface SystemInfo {
  hostname: string
  os: string
  kernel: string
  arch: string
  cpu_model: string
  cpu_cores: number
  mem_total: number
  swap_total: number
  disk_total: number
  virt: string
  boot_time: number
}

export interface ServerView {
  id: string
  name: string
  group_id: number | null
  online: boolean
  last_seen: number
  expires_at: number | null
  purchased_at: number | null
  traffic_reset_day: number | null
  pay_cycle_months: number | null
  price_per_cycle: number | null
  currency: string | null
  region: string | null
  /** 线路标注（如 CN2 GIA） */
  line: string | null
  /** 带宽（Mbps） */
  bandwidth_mbps: number | null
  ipv4: string | null
  ipv6: string | null
  /** 商家备注（如 VPS BY DMIT.io） */
  remark: string | null
  /** agent 最近连接 IP */
  last_ip: string | null
  public_visible: boolean
  system_info: SystemInfo | null
  metrics: RealtimeMetrics | null
  traffic_quota_bytes: number | null
  month_traffic: number
}

export type DashboardEvent =
  | { type: 'snapshot'; servers: ServerView[] }
  | { type: 'update'; server: ServerView }
  | { type: 'offline'; id: string }
  | { type: 'resort'; ids: string[] }

// 当前登录用户（/api/auth/me）
export interface Me {
  username: string
  role: string // 'admin' | 'viewer'
  group_ids: number[]
}
