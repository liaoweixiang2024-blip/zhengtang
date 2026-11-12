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
        // 公开展示页（匿名，仅 public_visible 的脱敏子集）
        .route("/api/public/servers", get(public_servers))
}

async fn status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let user_count = state
        .store
        .with(|c| Ok(c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))?))
        .unwrap_or(0);
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "protocol_version": proto::PROTOCOL_VERSION,
        // 首启向导判定：无用户时前端引导创建管理员
        "setup_needed": user_count == 0,
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

    let total_bytes = state
        .store
        .with(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(bytes_in,0) + COALESCE(bytes_out,0) FROM traffic_monthly WHERE server_id = ?1 AND month = ?2",
                rusqlite::params![id, current_month()],
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


/// 公开展示：仅 public_visible 服务器；不含 IP/系统详情/到期价等敏感字段。
async fn public_servers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
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
    (StatusCode::OK, Json(json!({ "servers": views })))
}
