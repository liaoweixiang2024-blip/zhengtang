//! 全局共享状态：实时在线表（内存）+ 仪表盘广播。

use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use proto::{RealtimeMetrics, SystemInfo};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::store::{unix_now, Store};

/// 一台已连接 agent 的实时状态。
pub struct LiveAgent {
    pub connected_at: Instant,
    pub last_metrics_at: Instant,
    pub last_seen: Instant,
}

/// 广播给仪表盘 WS 的增量事件（JSON over WS）。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DashboardEvent {
    /// 单台服务器状态/指标更新
    Update { server: ServerView },
    /// 单台服务器离线
    Offline { id: String },
}

/// /api/servers 与仪表盘推送共用的服务器视图。
#[derive(Debug, Clone, Serialize)]
pub struct ServerView {
    pub id: String,
    pub name: String,
    pub group_id: Option<i64>,
    pub online: bool,
    pub last_seen: i64,
    pub expires_at: Option<i64>,
    /// 购买日（unix 秒）
    pub purchased_at: Option<i64>,
    /// 流量重置日（1-28；None=自然月）
    pub traffic_reset_day: Option<i64>,
    /// 付费周期（月数：1/3/12/36/60）
    pub pay_cycle_months: Option<i64>,
    /// 每期费用
    pub price_per_cycle: Option<f64>,
    /// 货币（CNY/USD/EUR/JPY/HKD/GBP）
    pub currency: Option<String>,
    /// 地区徽章（如 '🇭🇰 香港'）
    pub region: Option<String>,
    pub public_visible: bool,
    pub system_info: Option<SystemInfo>,
    pub metrics: Option<RealtimeMetrics>,
    pub traffic_quota_bytes: Option<i64>,
    /// 本账期已用流量（bytes，按流量重置日对齐的账期）
    pub month_traffic: i64,
}

/// 站点设置（DB settings 表，带默认值）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SiteSettings {
    pub site_name: String,
    pub site_description: String,
    /// 公开展示页总开关
    pub public_page_enabled: bool,
}

impl Default for SiteSettings {
    fn default() -> Self {
        Self {
            site_name: "zhengtang".into(),
            site_description: "服务器在线状态与资源监控".into(),
            public_page_enabled: true,
        }
    }
}

/// 静态注册信息（DB 读出，低频刷新）。
#[derive(Debug, Clone)]
pub struct ServerReg {
    pub id: String,
    pub name: String,
    pub group_id: Option<i64>,
    pub expires_at: Option<i64>,
    pub public_visible: bool,
    pub traffic_quota_bytes: Option<i64>,
    pub purchased_at: Option<i64>,
    pub traffic_reset_day: Option<i64>,
    pub pay_cycle_months: Option<i64>,
    pub price_per_cycle: Option<f64>,
    pub currency: Option<String>,
    pub region: Option<String>,
}

pub struct AppState {
    pub store: Arc<Store>,
    /// 登录防爆破（指数退避）
    pub brute: crate::auth::BruteGuard,
    /// agent_id → 实时状态
    pub live: DashMap<String, LiveAgent>,
    /// 最近一次指标（含离线机器最后的快照，重启丢失——低价值数据可接受）
    pub metrics: DashMap<String, RealtimeMetrics>,
    pub system_info: DashMap<String, SystemInfo>,
    /// 最近 ~70s 的 CPU 样本（1min 聚合用）
    pub cpu_history: DashMap<String, std::collections::VecDeque<(std::time::Instant, f32)>>,
    /// 在线 agent 的命令下发通道（拨测/进程规则变更推送）
    pub agent_cmds: DashMap<String, tokio::sync::mpsc::UnboundedSender<proto::ServerMsg>>,
    /// 注册表缓存（add-server / 删除时 invalidate + reload）
    pub registry: dashmap::DashMap<String, [u8; 32]>,
    pub dash_tx: broadcast::Sender<DashboardEvent>,
    /// agent 指标上报间隔（秒），握手通过后下发
    pub metrics_interval_secs: u32,
}

impl AppState {
    pub fn new(store: Arc<Store>, metrics_interval_secs: u32) -> Result<Self, anyhow::Error> {
        let (dash_tx, _) = broadcast::channel(256);
        let state = Self {
            store,
            brute: Default::default(),
            live: DashMap::new(),
            metrics: DashMap::new(),
            system_info: DashMap::new(),
            cpu_history: DashMap::new(),
            agent_cmds: DashMap::new(),
            registry: DashMap::new(),
            dash_tx,
            metrics_interval_secs,
        };
        state.reload_registry()?;
        Ok(state)
    }

    /// 记录一次 CPU 样本（保留 70s 窗口）。
    pub fn push_cpu(&self, id: &str, usage: f32) {
        let mut q = self.cpu_history.entry(id.to_string()).or_default();
        let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(70);
        while q.front().is_some_and(|(t, _)| *t < cutoff) {
            q.pop_front();
        }
        q.push_back((std::time::Instant::now(), usage));
    }

    /// 当前窗口 CPU 统计 (min, avg, max)；无样本返回 0。
    pub fn cpu_stats(&self, id: &str) -> (f32, f32, f32) {
        match self.cpu_history.get(id) {
            Some(q) if !q.is_empty() => {
                let mut min = f32::MAX;
                let mut max = f32::MIN;
                let mut sum = 0.0;
                for (_, v) in q.iter() {
                    min = min.min(*v);
                    max = max.max(*v);
                    sum += v;
                }
                (min, sum / q.len() as f32, max)
            }
            _ => (0.0, 0.0, 0.0),
        }
    }

    /// 从 DB 加载 agent_id → pubkey 注册表（启动时 + 变更时）。
    pub fn reload_registry(&self) -> Result<(), anyhow::Error> {
        let entries = self.store.with(|c| {
            let mut stmt = c.prepare("SELECT id, pubkey FROM servers")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        self.registry.clear();
        for (id, pk) in entries {
            if pk.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&pk);
                self.registry.insert(id, arr);
            }
        }
        tracing::info!(count = self.registry.len(), "agent 注册表已加载");
        Ok(())
    }

    pub fn online_ids(&self) -> Vec<String> {
        self.live.iter().map(|e| e.key().clone()).collect()
    }

    /// 构建完整服务器视图（合并注册信息 + 实时状态 + 本账期流量）。
    pub fn server_views(&self) -> Vec<ServerView> {
        self.store
            .with(|c| {
                let rows = {
                    let mut stmt = c.prepare(
                        "SELECT id, name, group_id, expires_at, public_visible, traffic_quota_bytes, COALESCE(last_seen_at, 0), purchased_at, traffic_reset_day, pay_cycle_months, price_per_cycle, currency, region FROM servers ORDER BY created_at",
                    )?;
                    let collected: Vec<_> = stmt
                        .query_map([], |r| {
                            Ok((ServerReg {
                                id: r.get(0)?,
                                name: r.get(1)?,
                                group_id: r.get(2)?,
                                expires_at: r.get(3)?,
                                public_visible: r.get::<_, i64>(4)? != 0,
                                traffic_quota_bytes: r.get(5)?,
                                purchased_at: r.get(7)?,
                                traffic_reset_day: r.get(8)?,
                                pay_cycle_months: r.get(9)?,
                                price_per_cycle: r.get(10)?,
                                currency: r.get(11)?,
                                region: r.get(12)?,
                            }, r.get::<_, i64>(6)?))
                        })?
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    collected
                };
                // 本账期流量（按各机流量重置日对齐的 billing_period 归月）
                let mut traffic = Vec::with_capacity(rows.len());
                for (reg, _) in &rows {
                    let period = crate::jobs::billing_period(unix_now(), reg.traffic_reset_day);
                    let used: i64 = c
                        .query_row(
                            "SELECT COALESCE(bytes_in,0) + COALESCE(bytes_out,0) FROM traffic_monthly WHERE server_id = ?1 AND month = ?2",
                            rusqlite::params![reg.id, period],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);
                    traffic.push(used);
                }
                Ok(rows.into_iter().zip(traffic).collect::<Vec<_>>())
            })
            .unwrap_or_default()
            .into_iter()
            .map(|((reg, db_last_seen), month_traffic)| {
                let online = self.live.contains_key(&reg.id);
                let last_seen = if online { unix_now() } else { db_last_seen };
                ServerView {
                    id: reg.id.clone(),
                    name: reg.name,
                    group_id: reg.group_id,
                    online,
                    last_seen,
                    expires_at: reg.expires_at,
                    purchased_at: reg.purchased_at,
                    traffic_reset_day: reg.traffic_reset_day,
                    pay_cycle_months: reg.pay_cycle_months,
                    price_per_cycle: reg.price_per_cycle,
                    currency: reg.currency,
                    region: reg.region,
                    public_visible: reg.public_visible,
                    system_info: self.system_info.get(&reg.id).map(|s| s.clone()),
                    metrics: self.metrics.get(&reg.id).map(|m| m.clone()),
                    traffic_quota_bytes: reg.traffic_quota_bytes,
                    month_traffic,
                }
            })
            .collect()
    }

    pub fn server_view(&self, id: &str) -> Option<ServerView> {
        self.server_views().into_iter().find(|v| v.id == id)
    }

    pub fn settings(&self) -> SiteSettings {
        let mut s = SiteSettings::default();
        let _ = self.store.with(|c| {
            let get = |k: &str| -> Option<String> {
                c.query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| r.get(0)).ok()
            };
            if let Some(v) = get("site_name") { s.site_name = v; }
            if let Some(v) = get("site_description") { s.site_description = v; }
            if let Some(v) = get("public_page_enabled") { s.public_page_enabled = v != "0"; }
            Ok(())
        });
        s
    }

    pub fn set_settings(&self, s: &SiteSettings) {
        let _ = self.store.with(|c| {
            for (k, v) in [
                ("site_name", s.site_name.clone()),
                ("site_description", s.site_description.clone()),
                ("public_page_enabled", if s.public_page_enabled { "1".into() } else { "0".into() }),
            ] {
                c.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = ?2",
                    rusqlite::params![k, v],
                )?;
            }
            Ok(())
        });
    }

    /// 持久化 last_seen（低频：每分钟由 gateway 调用，避免写放大）。
    pub fn persist_last_seen(&self, ids: &[String]) {
        let _ = self.store.with(|c| {
            for id in ids {
                let _ = c.execute(
                    "UPDATE servers SET last_seen_at = ?1 WHERE id = ?2",
                    rusqlite::params![unix_now(), id],
                );
            }
            Ok(())
        });
    }

    /// agent 上线：记录 last_seen 与对端 IP（地区自动识别用）。
    pub fn persist_agent_seen(&self, id: &str, ip: &str) {
        let _ = self.store.with(|c| {
            let _ = c.execute(
                "UPDATE servers SET last_seen_at = ?1, last_ip = ?2 WHERE id = ?3",
                rusqlite::params![unix_now(), ip, id],
            );
            Ok(())
        });
    }
}

impl AppState {
    /// 某台服务器的拨测任务（转 proto 结构下发）。
    pub fn dial_tasks(&self, server_id: &str) -> Vec<proto::DialTask> {
        self.store
            .with(|c| {
                let mut stmt = c.prepare(
                    "SELECT id, kind, target, interval_secs, timeout_ms FROM dial_tasks
                     WHERE server_id = ?1 AND enabled = 1",
                )?;
                let rows = stmt
                    .query_map([server_id], |r| {
                        let kind: String = r.get(1)?;
                        let target: String = r.get(2)?;
                        let k = match kind.as_str() {
                            "http" => proto::DialKind::HttpGet {
                                url: target,
                                expect_status: None,
                            },
                            _ => {
                                // target 格式 host:port
                                let (h, p) = target.rsplit_once(':').unwrap_or((target.as_str(), "80"));
                                proto::DialKind::TcpConnect { host: h.to_string(), port: p.parse().unwrap_or(80) }
                            }
                        };
                        Ok(proto::DialTask {
                            id: r.get(0)?,
                            kind: k,
                            interval_secs: r.get::<_, i64>(3)? as u32,
                            timeout_ms: r.get::<_, i64>(4)? as u32,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap_or_default()
    }

    /// 向在线 agent 推送其拨测任务全集。
    pub fn push_dial_tasks(&self, server_id: &str) {
        if let Some(tx) = self.agent_cmds.get(server_id) {
            let _ = tx.send(ServerMsg::DialTasks(self.dial_tasks(server_id)));
        }
    }

    /// 某台服务器的进程监控规则（转 proto 下发）。
    pub fn process_rules(&self, server_id: &str) -> Vec<proto::ProcessRule> {
        self.store
            .with(|c| {
                let mut stmt = c.prepare(
                    "SELECT id, name, exact FROM process_rules WHERE server_id = ?1",
                )?;
                let rows = stmt
                    .query_map([server_id], |r| {
                        Ok(proto::ProcessRule {
                            id: r.get(0)?,
                            name: r.get(1)?,
                            exact: r.get::<_, i64>(2)? != 0,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap_or_default()
    }

    /// 推送进程规则全集。
    pub fn push_process_rules(&self, server_id: &str) {
        if let Some(tx) = self.agent_cmds.get(server_id) {
            let _ = tx.send(ServerMsg::ProcessRules(self.process_rules(server_id)));
        }
    }
}

use proto::ServerMsg;
