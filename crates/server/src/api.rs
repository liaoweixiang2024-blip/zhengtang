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

async fn servers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO(M4): 按会话用户组授权过滤；当前开发阶段直接返回全量
    let views = state.server_views();
    (StatusCode::OK, Json(json!({ "servers": views })))
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
