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
    pub public_visible: bool,
    pub system_info: Option<SystemInfo>,
    pub metrics: Option<RealtimeMetrics>,
    pub traffic_quota_bytes: Option<i64>,
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

    /// 构建完整服务器视图（合并注册信息 + 实时状态）。
    pub fn server_views(&self) -> Vec<ServerView> {
        self.store
            .with(|c| {
                let mut stmt = c.prepare(
                    "SELECT id, name, group_id, expires_at, public_visible, traffic_quota_bytes, COALESCE(last_seen_at, 0) FROM servers ORDER BY created_at",
                )?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((ServerReg {
                            id: r.get(0)?,
                            name: r.get(1)?,
                            group_id: r.get(2)?,
                            expires_at: r.get(3)?,
                            public_visible: r.get::<_, i64>(4)? != 0,
                            traffic_quota_bytes: r.get(5)?,
                        }, r.get::<_, i64>(6)?))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap_or_default()
            .into_iter()
            .map(|(reg, db_last_seen)| {
                let online = self.live.contains_key(&reg.id);
                let last_seen = if online { unix_now() } else { db_last_seen };
                ServerView {
                    id: reg.id.clone(),
                    name: reg.name,
                    group_id: reg.group_id,
                    online,
                    last_seen,
                    expires_at: reg.expires_at,
                    public_visible: reg.public_visible,
                    system_info: self.system_info.get(&reg.id).map(|s| s.clone()),
                    metrics: self.metrics.get(&reg.id).map(|m| m.clone()),
                    traffic_quota_bytes: reg.traffic_quota_bytes,
                }
            })
            .collect()
    }

    pub fn server_view(&self, id: &str) -> Option<ServerView> {
        self.server_views().into_iter().find(|v| v.id == id)
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
}
