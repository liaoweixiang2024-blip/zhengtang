//! 告警引擎：30s 周期求值内建规则 → 通知渠道（Telegram/Bark/邮件/webhook）。
//!
//! 状态机 per (rule, server)：Idle → Firing(alert_event) → (恢复) Idle。
//! Firing 期间不重复通知（恢复时发恢复通知）；notifications 表留发送历史。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde_json::json;

use crate::state::AppState;
use crate::store::unix_now;

#[derive(Debug, Clone)]
struct Rule {
    id: i64,
    server_id: Option<String>, // None = 全局（对每台生效）
    type_: String,             // offline|cpu|mem|disk|expiry|traffic
    threshold: f64,            // cpu/mem/disk: %；expiry: 剩余天数；traffic: 配额百分比
    duration_secs: i64,
    silence_secs: i64,
}

#[derive(Default)]
struct EngineState {
    /// (rule_id, server_id) → 条件首次满足时间
    triggered_at: DashMap<(i64, String), Instant>,
    /// (rule_id, server_id) → 未满足时间（用于持续时长判定）
    clear_at: DashMap<(i64, String), Instant>,
}

pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        // 每 30s 求值一轮
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await;
        let engine = EngineState::default();
        loop {
            ticker.tick().await;
            match evaluate_all(&state, &engine).await {
                Ok(()) => {}
                Err(e) => tracing::warn!(error = %e, "告警求值失败"),
            }
            tracing::debug!("告警求值周期完成 rules={}", engine.triggered_at.len());
        }
    });
}

async fn evaluate_all(state: &Arc<AppState>, engine: &EngineState) -> anyhow::Result<()> {
    // 注册服务器 + 名称
    let servers: Vec<(String, String, Option<i64>, Option<i64>, Option<i64>)> = state
        .store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, expires_at, traffic_quota_bytes, last_seen_at FROM servers",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
    if servers.is_empty() {
        return Ok(());
    }

    // 默认规则（每台自动）：offline / cpu>90 / mem>90 / disk>90 / expiry<7天
    ensure_default_rules(state, &servers).await;

    let rules = load_rules(state)?;

    for rule in &rules {
        for (sid, name, expires_at, quota, last_seen) in &servers {
            // 从未上过线的机器不判离线（避免注册即告警）
            if rule.type_ == "offline" && last_seen.unwrap_or(0) == 0 {
                continue;
            }
            if let Some(rs) = &rule.server_id {
                if rs != sid {
                    continue;
                }
            }
            let key = (rule.id, sid.clone());
            let satisfied = match rule.type_.as_str() {
                "offline" => Some(!state.live.contains_key(sid)),
                "cpu" => state.metrics.get(sid).map(|m| m.cpu_usage as f64 > rule.threshold),
                "mem" => state.metrics.get(sid).and_then(|m| {
                    if m.mem_total > 0 {
                        Some(m.mem_used as f64 / m.mem_total as f64 * 100.0 > rule.threshold)
                    } else {
                        None
                    }
                }),
                "disk" => state.metrics.get(sid).and_then(|m| {
                    let root = m.disks.iter().find(|d| d.mount == "/").or_else(|| m.disks.first())?;
                    if root.total > 0 {
                        Some(root.used as f64 / root.total as f64 * 100.0 > rule.threshold)
                    } else {
                        None
                    }
                }),
                "expiry" => expires_at.map(|e| {
                    let days = (e - unix_now()) as f64 / 86400.0;
                    days < rule.threshold
                }),
                "traffic" => quota.and_then(|q| {
                    if q <= 0 {
                        return None;
                    }
                    let month = crate::jobs::year_month_from_days(unix_now() / 86400);
                    let month_str = format!("{:04}-{:02}", month.0, month.1);
                    let used: i64 = state
                        .store
                        .with(|c| {
                            Ok(c
                                .query_row(
                                    "SELECT bytes_in + bytes_out FROM traffic_monthly WHERE server_id = ?1 AND month = ?2",
                                    rusqlite::params![sid, month_str],
                                    |r| r.get::<_, i64>(0),
                                )
                                .unwrap_or(0))
                        })
                        .unwrap_or(0);
                    Some(used as f64 / q as f64 * 100.0 > rule.threshold)
                }),
                _ => None,
            };

            match satisfied {
                None => {} // 数据不可得：跳过
                Some(true) => {
                    engine.clear_at.remove(&key);
                    let first = *engine
                        .triggered_at
                        .entry(key.clone())
                        .or_insert_with(Instant::now);
                    // 需持续 duration（首次触发立即记录，持续窗口用 duration 判定）
                    if first.elapsed().as_secs() >= rule.duration_secs as u64 {
                        fire(state, rule, sid, name, engine).await;
                    }
                }
                Some(false) => {
                    engine.triggered_at.remove(&key);
                    resolve(state, rule, sid, name, engine).await;
                }
            }
        }
    }
    Ok(())
}

/// 触发（幂等：已有未恢复事件则跳过）。
async fn fire(state: &Arc<AppState>, rule: &Rule, sid: &str, name: &str, engine: &EngineState) {
    let existing = state
        .store
        .with(|c| {
            Ok(c.query_row(
                "SELECT id FROM alert_events WHERE rule_id = ?1 AND server_id = ?2 AND resolved_at IS NULL",
                rusqlite::params![rule.id, sid],
                |r| r.get::<_, i64>(0),
            )
            .ok())
        })
        .unwrap_or(None);
    if existing.is_some() {
        return; // 已在告警中（静默）
    }

    let summary = match rule.type_.as_str() {
        "offline" => format!("{name} 离线"),
        "cpu" => format!("{name} CPU 超过 {threshold:.0}%", threshold = rule.threshold),
        "mem" => format!("{name} 内存超过 {threshold:.0}%", threshold = rule.threshold),
        "disk" => format!("{name} 磁盘超过 {threshold:.0}%", threshold = rule.threshold),
        "expiry" => format!("{name} 即将到期（{threshold:.0} 天内）", threshold = rule.threshold),
        "traffic" => format!("{name} 月流量超过配额 {threshold:.0}%", threshold = rule.threshold),
        t => format!("{name} 触发 {t}"),
    };

    let _ = state.store.with(|c| {
        c.execute(
            "INSERT INTO alert_events (rule_id, server_id, fired_at, summary) VALUES (?1,?2,?3,?4)",
            rusqlite::params![rule.id, sid, unix_now(), summary],
        )?;
        Ok(())
    });
    tracing::warn!(rule = rule.type_, server = name, "告警触发: {summary}");
    dispatch_all(state, &format!("🚨 {summary}"), "zhengtang 告警").await;
    engine.triggered_at.insert((rule.id, sid.to_string()), Instant::now());
}

/// 恢复（存在未恢复事件则写 resolved_at + 发恢复通知）。
async fn resolve(state: &Arc<AppState>, rule: &Rule, sid: &str, name: &str, _engine: &EngineState) {
    let existing = state
        .store
        .with(|c| {
            Ok(c.query_row(
                "SELECT id, summary FROM alert_events WHERE rule_id = ?1 AND server_id = ?2 AND resolved_at IS NULL",
                rusqlite::params![rule.id, sid],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .ok())
        })
        .unwrap_or(None);
    let Some((event_id, summary)) = existing else {
        return;
    };
    let _ = state.store.with(|c| {
        c.execute(
            "UPDATE alert_events SET resolved_at = ?1 WHERE id = ?2",
            rusqlite::params![unix_now(), event_id],
        )?;
        Ok(())
    });
    tracing::info!(server = name, "告警恢复: {summary}");
    dispatch_all(state, &format!("✅ 已恢复：{summary}"), "zhengtang 恢复").await;
}

/// 服务器注册时确保默认规则存在。
async fn ensure_default_rules(state: &Arc<AppState>, servers: &[(String, String, Option<i64>, Option<i64>, Option<i64>)]) {
    let _ = state.store.with(|c| {
        for (sid, ..) in servers {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM alert_rules WHERE server_id = ?1",
                [sid],
                |r| r.get(0),
            )?;
            if n == 0 {
                let defaults: [(&str, f64); 5] = [
                    ("offline", 0.0),
                    ("cpu", 90.0),
                    ("mem", 90.0),
                    ("disk", 90.0),
                    ("expiry", 7.0),
                ];
                for (t, th) in defaults {
                    c.execute(
                        "INSERT INTO alert_rules (server_id, type, params, duration_secs, silence_secs, enabled)
                         VALUES (?1, ?2, ?3, 60, 300, 1)",
                        rusqlite::params![sid, t, json!({"threshold": th}).to_string()],
                    )?;
                }
            }
        }
        Ok(())
    });
}

fn load_rules(state: &Arc<AppState>) -> anyhow::Result<Vec<Rule>> {
    state.store.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, server_id, type, params, duration_secs, silence_secs FROM alert_rules WHERE enabled = 1",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let params: String = r.get(3)?;
                let threshold = serde_json::from_str::<serde_json::Value>(&params)
                    .ok()
                    .and_then(|v| v.get("threshold").and_then(|t| t.as_f64()))
                    .unwrap_or(90.0);
                Ok(Rule {
                    id: r.get(0)?,
                    server_id: r.get(1)?,
                    type_: r.get(2)?,
                    threshold,
                    duration_secs: r.get(4)?,
                    silence_secs: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

// ---------- 通知渠道 ----------

pub struct Channel {
    pub id: i64,
    pub type_: String,
    pub config: serde_json::Value,
}

fn load_channels(state: &Arc<AppState>) -> Vec<Channel> {
    state
        .store
        .with(|c| {
            let mut stmt = c.prepare("SELECT id, type, config FROM notify_channels")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Channel {
                        id: r.get(0)?,
                        type_: r.get(1)?,
                        config: serde_json::from_str(&r.get::<_, String>(2)?)
                            .unwrap_or(serde_json::Value::Null),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default()
}

/// 向所有渠道并发发送（限制并发 4），写 notifications 历史。
pub async fn dispatch_all(state: &Arc<AppState>, body: &str, title: &str) {
    let channels = load_channels(state);
    if channels.is_empty() {
        return;
    }
    let sem = Arc::new(tokio::sync::Semaphore::new(4));
    let title = title.to_string();
    let mut handles = Vec::new();
    for ch in channels {
        let permit = sem.clone().acquire_owned().await;
        let body = body.to_string();
        let title = title.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            let ok = send_one(&ch, &body, &title).await;
            (ch.id, body, ok)
        }));
    }
    let results = futures_util_helper_join(handles).await;
    let _ = state.store.with(|c| {
        for (channel_id, body, ok) in results {
            c.execute(
                "INSERT INTO notifications (channel_id, title, body, status, created_at) VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![channel_id, title, body, if ok { "sent" } else { "failed" }, unix_now()],
            )?;
        }
        Ok(())
    });
}

/// 简易 join（无 futures 依赖也能用）。
async fn futures_util_helper_join(
    handles: Vec<tokio::task::JoinHandle<(i64, String, bool)>>,
) -> Vec<(i64, String, bool)> {
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(r) = h.await {
            out.push(r);
        }
    }
    out
}

/// 单渠道发送。config 字段见各类型。
pub async fn send_one(ch: &Channel, body: &str, _title: &str) -> bool {
    let result = match ch.type_.as_str() {
        // config: {"bot_token": "...", "chat_id": "..."}
        "telegram" => telegram(ch, body).await,
        // config: {"server": "https://api.day.app", "key": "..."}
        "bark" => bark(ch, body).await,
        // config: {"url": "https://..."}
        "webhook" => webhook(ch, body).await,
        // config: {"smtp_host": "...", "smtp_port": 465, "username": "...", "password": "...", "to": "a@b.c"}
        "email" => email(ch, body).await,
        t => Err(format!("未知渠道类型 {t}")),
    };
    match result {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(channel = ch.id, error = %e, "通知发送失败");
            false
        }
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client")
}

async fn telegram(ch: &Channel, body: &str) -> Result<(), String> {
    let token = ch.config.get("bot_token").and_then(|v| v.as_str()).ok_or("缺 bot_token")?;
    let chat = ch.config.get("chat_id").and_then(|v| v.as_str()).ok_or("缺 chat_id")?;
    http()
        .post(format!("https://api.telegram.org/bot{token}/sendMessage"))
        .json(&json!({ "chat_id": chat, "text": body }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn bark(ch: &Channel, body: &str) -> Result<(), String> {
    let server = ch
        .config
        .get("server")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.day.app")
        .trim_end_matches('/')
        .to_string();
    let key = ch.config.get("key").and_then(|v| v.as_str()).ok_or("缺 key")?;
    http()
        .post(format!("{server}/push"))
        .json(&json!({ "device_key": key, "title": "zhengtang", "body": body, "group": "zhengtang" }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn webhook(ch: &Channel, body: &str) -> Result<(), String> {
    let url = ch.config.get("url").and_then(|v| v.as_str()).ok_or("缺 url")?;
    http()
        .post(url)
        .json(&json!({ "title": "zhengtang", "body": body, "ts": unix_now() }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn email(ch: &Channel, body: &str) -> Result<(), String> {
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
    let host = ch.config.get("smtp_host").and_then(|v| v.as_str()).ok_or("缺 smtp_host")?;
    let port = ch.config.get("smtp_port").and_then(|v| v.as_u64()).unwrap_or(465) as u16;
    let user = ch.config.get("username").and_then(|v| v.as_str()).ok_or("缺 username")?;
    let pass = ch.config.get("password").and_then(|v| v.as_str()).ok_or("缺 password")?;
    let to = ch.config.get("to").and_then(|v| v.as_str()).ok_or("缺 to")?;

    let mail = Message::builder()
        .from(user.parse().map_err(|e| format!("发件人无效: {e:?}"))?)
        .to(to.parse().map_err(|e| format!("收件人无效: {e:?}"))?)
        .subject("zhengtang 告警")
        .body(body.to_string())
        .map_err(|e| e.to_string())?;
    let mailer: AsyncSmtpTransport<Tokio1Executor> = if port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(host)
            .map_err(|e| e.to_string())?
            .credentials((user, pass).into())
            .port(port)
            .build()
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
            .map_err(|e| e.to_string())?
            .credentials((user, pass).into())
            .port(port)
            .build()
    };
    mailer.send(mail).await.map_err(|e| e.to_string())?;
    Ok(())
}

// HashMap 保留给未来状态扩展
#[allow(dead_code)]
type _Unused = HashMap<String, String>;

/// 从 DB 行构造 Channel（auth_api 测试发送用）。
pub fn channel_from_row(id: i64, type_: String, config_json: String) -> Channel {
    Channel {
        id,
        type_,
        config: serde_json::from_str(&config_json).unwrap_or(serde_json::Value::Null),
    }
}
