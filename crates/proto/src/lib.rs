//! zhengtang 探针协议定义（Agent ↔ Server 共享）。
//!
//! # 安全不变量（SECURITY INVARIANT）
//!
//! 本文件定义的 [`ServerMsg`] 枚举是 Server 能发给 Agent 的**全部**指令集合。
//! 它永远不允许包含任何具备以下能力的变体或字段：
//!
//! - 执行命令 / 启动进程
//! - 读写任意文件 / 文件传输
//! - 加载代码 / 配置除本枚举列明的采集参数之外的任何行为
//!
//! Agent 端唯一的"动作"是：采集指标、执行受约束的拨测（仅 HTTP GET /
//! TCP connect，带超时与体积上限）、上报结果。协议层面不存在任何执行原语，
//! 因此即使 Server 被完全攻破，被监控机也不会被入侵。
//! CI 脚本 `scripts/check-protocol.sh` 对此做机器检查。
//!
//! 序列化使用 postcard（紧凑、无反序列化放大攻击面）。

use serde::{Deserialize, Serialize};

/// 协议版本。不兼容变更时递增；Server 按此字段决定拒接或降级。
pub const PROTOCOL_VERSION: u8 = 1;

/// 单条 WS 消息的硬上限（Metrics 含每核 CPU 与磁盘列表，256KB 足够）。
pub const MAX_MESSAGE_SIZE: usize = 256 * 1024;

/// 握手时间戳允许的时钟偏移窗口（秒）。
pub const HANDSHAKE_TIMESTAMP_WINDOW_SECS: u64 = 60;

/// Agent → Server 消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentMsg {
    /// 握手：身份 + 对 challenge 的 Ed25519 签名。
    ///
    /// 签名内容 = `challenge || agent_id.as_bytes() || timestamp.to_le_bytes()`，
    /// 私钥永不上线。
    Hello {
        agent_id: String,
        /// Ed25519 公钥（32 字节）
        pubkey: [u8; 32],
        /// unix 秒，须在 ±[`HANDSHAKE_TIMESTAMP_WINDOW_SECS`] 内
        timestamp: u64,
        /// Ed25519 签名（64 字节）
        sig: Vec<u8>,
        /// agent 支持的协议版本
        proto_version: u8,
    },
    /// 实时指标（默认 ~3s 一次，间隔由 Server 在 HelloOk 中下发）。
    Metrics(RealtimeMetrics),
    /// 低频完整报告（开机/每 5min/Server 要求时）。
    FullReport(Box<SystemInfo>),
    /// 拨测结果。
    DialResult {
        task_id: i64,
        ok: bool,
        latency_ms: u32,
        /// HTTP 状态码或错误摘要（如 "200"、"timeout"、"refused"）
        status: String,
    },
    /// 对 Server Ping 的回应。
    Pong { nonce: u64 },
}

/// Server → Agent 消息。
///
/// 见模块级安全不变量注释：本枚举穷尽了 Server 的全部控制能力，
/// 不存在任何命令执行 / 文件读写语义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMsg {
    /// 连接建立后立即下发的一次性握手挑战（32 随机字节）。
    Challenge { challenge: [u8; 32] },
    /// 握手通过。
    HelloOk {
        /// 指标上报间隔（秒）
        metrics_interval_secs: u32,
        server_time: u64,
    },
    /// 握手被拒（验签失败/未注册/时间窗外的具体原因不回传，防探测）。
    HelloErr,
    /// 保活探测。
    Ping { nonce: u64 },
    /// 拨测任务全集（整体替换，空列表 = 无任务）。
    /// 仅描述"对哪里做 HTTP GET / TCP connect"，无任何执行语义。
    DialTasks(Vec<DialTask>),
    /// 进程监控规则全集（整体替换）。
    /// 仅描述"按名字匹配进程并统计"，无任何控制语义。
    ProcessRules(Vec<ProcessRule>),
    /// 要求 Agent 立即重报一次 FullReport。
    RequestFullReport,
}

/// 受约束的拨测任务。Agent 只允许做 HTTP GET（禁重定向、body ≤ 4KB）
/// 或 TCP connect，带超时；不允许其他任何网络操作。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialTask {
    pub id: i64,
    pub kind: DialKind,
    /// 最小执行间隔（秒）
    pub interval_secs: u32,
    /// 单次超时（毫秒），上限 10000
    pub timeout_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DialKind {
    /// GET 一个 URL。Agent 禁跟随重定向、不发送 cookie、只读响应头与前 4KB。
    HttpGet { url: String, expect_status: Option<u16> },
    /// TCP 端口连通性检查。
    TcpConnect { host: String, port: u16 },
}

/// 进程监控规则：匹配则上报数量/CPU/内存。只读统计，无控制语义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessRule {
    pub id: i64,
    /// 进程名匹配串
    pub name: String,
    /// true = 精确匹配进程名；false = 子串匹配
    pub exact: bool,
}

/// 实时指标快照。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RealtimeMetrics {
    /// 总 CPU 使用率（0-100）
    pub cpu_usage: f32,
    /// 每核使用率
    pub cpu_per_core: Vec<f32>,
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub load1: f32,
    pub load5: f32,
    pub load15: f32,
    pub uptime_secs: u64,
    /// 网卡累计接收字节数（跨周期由 Server/前端差分；计数变小视为网卡重置）
    pub net_in_total: u64,
    pub net_out_total: u64,
    /// 当前速率（字节/秒，由 Agent 用相邻两次采样差分得出）
    pub net_in_speed: u64,
    pub net_out_speed: u64,
    /// 挂载点磁盘使用
    pub disks: Vec<DiskUsage>,
    /// GPU 指标（无 GPU 或不可读时为空）
    pub gpus: Vec<GpuInfo>,
    /// 命中的进程监控规则统计
    pub processes: Vec<ProcStat>,
    /// 进程总数
    pub process_count: u64,
    /// TCP 连接数 / UDP 连接数
    pub tcp_conn_count: u64,
    pub udp_conn_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskUsage {
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    /// 利用率 0-100
    pub utilization: f32,
    pub mem_total: u64,
    pub mem_used: u64,
    /// 温度 ℃（不可读为 -1）
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcStat {
    pub rule_id: i64,
    /// 匹配到的进程数
    pub count: u32,
    /// 匹配进程合计 CPU%（单核口径）
    pub cpu: f32,
    /// 匹配进程合计内存（字节）
    pub mem_bytes: u64,
}

/// 低频完整系统信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub hostname: String,
    /// 如 "Ubuntu 24.04" / "macOS 15.1"
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub mem_total: u64,
    pub swap_total: u64,
    pub disk_total: u64,
    /// 虚拟化类型（"kvm"/"openvz"/"none"/"unknown"）
    pub virt: String,
    pub boot_time: u64,
}

/// 握手签名原文：challenge || agent_id || timestamp(le)。
/// 两侧共用，保证字节级一致。
pub fn handshake_signing_payload(
    challenge: &[u8; 32],
    agent_id: &str,
    timestamp: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32 + agent_id.len() + 8);
    buf.extend_from_slice(challenge);
    buf.extend_from_slice(agent_id.as_bytes());
    buf.extend_from_slice(&timestamp.to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 协议往返序列化测试：保证 postcard 可正确 round-trip 所有消息。
    #[test]
    fn roundtrip_agent_msg() {
        let msg = AgentMsg::Hello {
            agent_id: "test-agent".into(),
            pubkey: [1; 32],
            timestamp: 42,
            sig: vec![2; 64],
            proto_version: PROTOCOL_VERSION,
        };
        let bytes = postcard::to_allocvec(&msg).unwrap();
        let back: AgentMsg = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(back, AgentMsg::Hello { ref agent_id, .. } if agent_id == "test-agent"));
    }

    #[test]
    fn roundtrip_metrics() {
        let m = RealtimeMetrics {
            cpu_usage: 12.5,
            mem_total: 1000,
            disks: vec![DiskUsage { mount: "/".into(), fs: "ext4".into(), total: 10, used: 5 }],
            ..Default::default()
        };
        let bytes = postcard::to_allocvec(&AgentMsg::Metrics(m)).unwrap();
        let back: AgentMsg = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(back, AgentMsg::Metrics(ref m) if m.cpu_usage == 12.5));
    }
}
