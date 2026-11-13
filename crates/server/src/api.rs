//! HTTP API（M1/M2：status + servers 列表；认证与更多端点在 M4+ 加入）。

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/servers", get(servers))
        .route("/api/servers/{id}/history", get(history))
        // 概览统计：指定时间起的流量合计（前端传本地今日 0 点）
        .route("/api/traffic/summary", get(traffic_summary))
        // 公开展示页（匿名，仅 public_visible 的脱敏子集）
        .route("/api/public/servers", get(public_servers))
        // 短安装命令：令牌换取已注入参数的安装脚本（匿名；令牌 24h 限时）
        .route("/a/{token}", get(install_script))
        // agent 二进制自托管下载（data/agents/，国内/GitHub 不可达环境用）
        .route("/dl/{file}", get(download_agent))
}

/// 从 data/agents/ 下发 agent 包（仅允许 zhengtang-agent-*.tar.gz 文件名）。
async fn download_agent(
    State(state): State<Arc<AppState>>,
    Path(file): Path<String>,
) -> impl IntoResponse {
    let ok = file.starts_with("zhengtang-agent-")
        && file.ends_with(".tar.gz")
        && !file.contains("..")
        && !file.contains('/');
    if !ok {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let path = state.data_dir.join("agents").join(&file);
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (axum::http::header::CONTENT_TYPE, "application/gzip"),
                (axum::http::header::CACHE_CONTROL, "public, max-age=3600"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "面板未自托管该文件（把 release 的 agent tar.gz 放入 data/agents/ 后重试）").into_response(),
    }
}

async fn status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let user_count = state
        .store
        .with(|c| Ok(c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))?))
        .unwrap_or(0);
    let st = state.settings();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "protocol_version": proto::PROTOCOL_VERSION,
        // 首启向导判定：无用户时前端引导创建管理员
        "setup_needed": user_count == 0,
        "site_name": st.site_name,
        "site_description": st.site_description,
        "public_page_enabled": st.public_page_enabled,
        "globe_enabled": st.globe_enabled,
    }))
}

async fn servers(
    State(state): State<Arc<AppState>>,
    jar: axum_extra::extract::CookieJar,
) -> impl IntoResponse {
    // 按会话用户授权过滤：admin 全量；viewer 仅其组内；未登录 401
    let Some(user) = crate::auth::current_user(&state, &jar) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "未登录" })),
        )
            .into_response();
    };
    let views: Vec<_> = state
        .server_views()
        .into_iter()
        .filter(|v| v.id == "x-nomatch" || server_visible_to(&user, v.group_id))
        .collect();
    (StatusCode::OK, Json(json!({ "servers": views }))).into_response()
}

/// viewer 只能看到其授权组内的服务器（未分组的服务器仅 admin 可见）。
pub fn server_visible_to(user: &crate::auth::AuthedUser, group_id: Option<i64>) -> bool {
    if user.role == "admin" {
        return true;
    }
    match group_id {
        Some(g) => user.group_ids.contains(&g),
        None => false,
    }
}

#[derive(Deserialize)]
struct HistoryQuery {
    /// 1h | 6h | 24h | 7d | 30d
    range: Option<String>,
}

/// 历史指标：≤24h 用 1min 粒度，≥7d 用 1h 粒度（降采样行）。
async fn history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> impl IntoResponse {
    let range = q.range.as_deref().unwrap_or("1h");
    let (secs, granularity): (i64, i64) = match range {
        "1h" => (3600, 1),
        "6h" => (6 * 3600, 1),
        "24h" => (24 * 3600, 1),
        "7d" => (7 * 86400, 2),
        "30d" => (30 * 86400, 2),
        _ => return (StatusCode::BAD_REQUEST, Json(json!({"error": "range 必须是 1h|6h|24h|7d|30d"}))).into_response(),
    };
    let since = crate::store::unix_now() - secs;

    let points = state
        .store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT ts, cpu_avg, mem_used, net_in, net_out, load1
                 FROM metric_samples
                 WHERE server_id = ?1 AND granularity = ?2 AND ts >= ?3
                 ORDER BY ts",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![id, granularity, since], |r| {
                    Ok(json!({
                        "ts": r.get::<_, i64>(0)?,
                        "cpu": r.get::<_, f64>(1)?,
                        "mem_used": r.get::<_, i64>(2)?,
                        "net_in": r.get::<_, i64>(3)?,
                        "net_out": r.get::<_, i64>(4)?,
                        "load1": r.get::<_, f64>(5)?,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();

    // 账期 key 按该机流量重置日对齐（与聚合侧一致）
    let period = state.store.with(|c| {
        let reset_day: Option<i64> = c
            .query_row("SELECT traffic_reset_day FROM servers WHERE id = ?1", [&id], |r| r.get(0))
            .ok()
            .flatten();
        Ok(crate::jobs::billing_period(crate::store::unix_now(), reset_day))
    }).unwrap_or_else(|_| current_month());
    let total_bytes = state
        .store
        .with(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(bytes_in,0) + COALESCE(bytes_out,0) FROM traffic_monthly WHERE server_id = ?1 AND month = ?2",
                rusqlite::params![id, period],
                |r| r.get::<_, i64>(0),
            ).unwrap_or(0))
        })
        .unwrap_or(0);

    (
        StatusCode::OK,
        Json(json!({ "points": points, "month_traffic": total_bytes })),
    )
        .into_response()
}

fn current_month() -> String {
    // 与 jobs::month_string 相同口径（UTC）
    let ts = crate::store::unix_now();
    let days = ts / 86400;
    let (y, m) = crate::jobs::year_month_from_days(days);
    format!("{y:04}-{m:02}")
}

#[derive(Deserialize)]
struct SummaryQuery {
    /// 起始 unix 秒（默认最近 24h；上限回看 35 天 = 1min 行保留期）
    since: Option<i64>,
}

/// 流量合计：对当前用户可见的服务器，按 since 起 1min 粒度样本求和（今日流量用）。
/// 返回总量 + 每机明细（概览卡悬停列表）。
async fn traffic_summary(
    State(state): State<Arc<AppState>>,
    jar: axum_extra::extract::CookieJar,
    Query(q): Query<SummaryQuery>,
) -> impl IntoResponse {
    let Some(user) = crate::auth::current_user(&state, &jar) else {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "未登录" }))).into_response();
    };
    let now = crate::store::unix_now();
    let since = q.since.unwrap_or(now - 86400).clamp(now - 35 * 86400, now);

    let visible: Vec<(String, String)> = state
        .server_views()
        .into_iter()
        .filter(|v| server_visible_to(&user, v.group_id))
        .map(|v| (v.id, v.name))
        .collect();
    if visible.is_empty() {
        return (StatusCode::OK, Json(json!({ "in": 0, "out": 0, "per_server": [] }))).into_response();
    }

    // 每机一次索引查询求和（机器数量级下开销可忽略，避免拼动态 IN 占位符）
    let rows = state
        .store
        .with(|c| {
            let mut rows: Vec<(String, i64, i64)> = Vec::new();
            for (id, name) in &visible {
                let (i, o): (i64, i64) = c
                    .query_row(
                        "SELECT COALESCE(SUM(net_in), 0), COALESCE(SUM(net_out), 0)
                         FROM metric_samples WHERE granularity = 1 AND ts >= ?1 AND server_id = ?2",
                        rusqlite::params![since, id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap_or((0, 0));
                rows.push((name.clone(), i, o));
            }
            rows.sort_by(|a, b| (b.1 + b.2).cmp(&(a.1 + a.2)));
            Ok(rows)
        })
        .unwrap_or_default();
    let bytes_in: i64 = rows.iter().map(|r| r.1).sum();
    let bytes_out: i64 = rows.iter().map(|r| r.2).sum();
    let per_server: Vec<_> = rows
        .into_iter()
        .take(12)
        .map(|(name, i, o)| json!({ "name": name, "in": i, "out": o }))
        .collect();
    (StatusCode::OK, Json(json!({ "in": bytes_in, "out": bytes_out, "per_server": per_server }))).into_response()
}



/// 一次性安装令牌 → 注入参数的 install.sh（curl -kfsSL .../a/<token> | sh）。
/// 令牌过期或服务器已删除返回 404/410。
async fn install_script(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(token): Path<String>,
) -> impl IntoResponse {
    // 参数顺序：id / 私钥 / 令牌过期时间（面板地址按请求 Host 推导）
    let row = state.store.with(|c| {
        Ok(c.query_row(
            "SELECT id, secret_key, install_token_expires FROM servers WHERE install_token = ?1",
            [&token],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            },
        )?)
    });
    let (agent_id, secret_key, expires) = match row {
        Ok(v) => v,
        Err(e) => { tracing::warn!(%e, "install token lookup failed"); return (StatusCode::NOT_FOUND, "安装令牌无效").into_response(); },
    };
    let Some(secret) = secret_key else {
        return (StatusCode::NOT_FOUND, "安装令牌无效").into_response();
    };
    if expires.unwrap_or(0) < crate::store::unix_now() {
        return (
            StatusCode::GONE,
            "安装令牌已过期（在面板删除并重新添加该服务器可获取新令牌）",
        )
            .into_response();
    }
    // 面板地址：按请求 Host 推导（反代场景取 X-Forwarded-Proto）
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1:8800");
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("https");
    let base = format!("{proto}://{host}");
    let ws = format!("{}/ws/agent", base.trim_end_matches('/').replace("https://", "wss://").replace("http://", "ws://"));
    let fingerprint = crate::tls::FINGERPRINT.get().map(|f| f.to_string()).unwrap_or_default();

    // 自托管 agent：data/agents/ 有任一 agent 包时，脚本改从面板下载（无需 GitHub）
    let has_local_agents = std::fs::read_dir(state.data_dir.join("agents"))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("zhengtang-agent-"))
        })
        .unwrap_or(false);
    let dl_base = if has_local_agents {
        format!("\nZT_DL_BASE='{base}/dl'")
    } else {
        String::new()
    };

    // 头部注入连接参数，其后为通用安装脚本（脚本内 ZT_* 为默认值，命令行参数可覆盖）
    let tpl = include_str!("../../../scripts/install.sh");
    let script = format!(
        "#!/usr/bin/env bash\n# ===== 面板注入（一次性安装令牌，勿外传）=====\nZT_SERVER='{ws}'\nZT_ID='{agent_id}'\nZT_KEY='{secret}'\nZT_FP='{fingerprint}'{dl_base}\n{tpl}"
    );
    (
        [(axum::http::header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        script,
    )
        .into_response()
}
/// 公开展示：仅 public_visible 服务器；不含 IP/系统详情/到期价等敏感字段。
async fn public_servers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // 总开关关闭时公开页不下发任何数据
    if !state.settings().public_page_enabled {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "公开页未开放" }))).into_response();
    }
    let views: Vec<_> = state
        .server_views()
        .into_iter()
        .filter(|v| v.public_visible)
        .map(|v| {
            let m = v.metrics;
            json!({
                "name": v.name,
                "online": v.online,
                "cpu_usage": m.as_ref().map(|m| m.cpu_usage),
                "mem_pct": m.as_ref().and_then(|m|
                    if m.mem_total > 0 { Some((m.mem_used as f64 / m.mem_total as f64) * 100.0) } else { None }),
                "net_in_speed": m.as_ref().map(|m| m.net_in_speed),
                "net_out_speed": m.as_ref().map(|m| m.net_out_speed),
                "uptime_secs": m.as_ref().map(|m| m.uptime_secs),
            })
        })
        .collect();
    (StatusCode::OK, Json(json!({ "servers": views }))).into_response()
}
