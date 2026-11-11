//! HTTP API（M1/M2：status + servers 列表；认证与更多端点在 M4+ 加入）。

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/servers", get(servers))
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
