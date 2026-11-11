//! Agent WebSocket 网关：challenge-response 握手 + 只读消息循环。
//!
//! 安全要点：
//! - 握手超时 10s；未注册 / 验签失败 / 时间窗外的连接一律 HelloErr 并关闭
//!   （不回传具体原因，防探测）
//! - 每连接消息大小受 axum ws 配置限制；Metrics 速率限制 ≥1s/条
//! - Server→Agent 的消息集合见 proto::ServerMsg（穷举、只读）

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, State, WebSocketUpgrade};
use axum::response::{IntoResponse, Response};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use proto::{
    handshake_signing_payload, AgentMsg, ServerMsg, HANDSHAKE_TIMESTAMP_WINDOW_SECS,
    MAX_MESSAGE_SIZE, PROTOCOL_VERSION,
};
use rand::RngCore;
use serde_json::json;
use tokio::sync::broadcast;

use crate::state::{AppState, DashboardEvent};

pub async fn ws_agent_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(_peer): ConnectInfo<std::net::SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.protocols(["zhengtang-agent-v1"])
        .max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: Arc<AppState>, mut socket: WebSocket) {
    // 1. 下发一次性 challenge
    let mut challenge = [0u8; 32];
    rand::rng().fill_bytes(&mut challenge);
    let hello = ServerMsg::Challenge { challenge };
    if send_proto(&mut socket, &hello).await.is_err() {
        return;
    }

    // 2. 等待 Hello（10s 超时）
    let agent_id = match wait_hello(&state, &mut socket, &challenge).await {
        Ok(id) => id,
        Err(reason) => {
            tracing::warn!(%reason, "agent 握手被拒");
            let _ = send_proto(&mut socket, &ServerMsg::HelloErr).await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };
    tracing::info!(agent = %agent_id, "agent 上线");

    // 3. 注册实时状态并回 HelloOk
    let now = Instant::now();
    state.live.insert(
        agent_id.clone(),
        crate::state::LiveAgent { connected_at: now, last_metrics_at: now, last_seen: now },
    );
    let ok = ServerMsg::HelloOk {
        metrics_interval_secs: state.metrics_interval_secs,
        server_time: crate::store::unix_now() as u64,
    };
    if send_proto(&mut socket, &ok).await.is_err() {
        state.live.remove(&agent_id);
        return;
    }
    if let Some(view) = state.server_view(&agent_id) {
        let _ = state.dash_tx.send(DashboardEvent::Update { server: view });
    }

    // 4. 消息循环（persist 用 interval：跨 select 保持计时，不受消息频率影响）
    let mut persist_ticker = tokio::time::interval(Duration::from_secs(60));
    persist_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    persist_ticker.tick().await; // 消费首个立即 tick
    loop {
        let msg = tokio::select! {
            m = socket.recv() => m,
            _ = persist_ticker.tick() => {
                state.persist_last_seen(&[agent_id.clone()]);
                continue;
            }
        };
        let Some(Ok(Message::Binary(data))) = msg else { break };
        if data.len() > MAX_MESSAGE_SIZE {
            tracing::warn!(agent = %agent_id, size = data.len(), "消息超限，断开");
            break;
        }
        let decoded: AgentMsg = match postcard::from_bytes(&data) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(agent = %agent_id, error = %e, "无法解码消息，丢弃");
                continue;
            }
        };
        match decoded {
            AgentMsg::Hello { .. } => {
                // 握手后重复 Hello：协议违规，断开
                tracing::warn!(agent = %agent_id, "握手后重复 Hello，断开");
                break;
            }
            AgentMsg::Metrics(m) => {
                // 速率限制：≥1s 一条
                let mut needs_broadcast = false;
                if let Some(mut live) = state.live.get_mut(&agent_id) {
                    if live.last_metrics_at.elapsed() >= Duration::from_secs(1) {
                        live.last_metrics_at = Instant::now();
                        live.last_seen = Instant::now();
                        needs_broadcast = true;
                    }
                }
                if needs_broadcast {
                    state.push_cpu(&agent_id, m.cpu_usage);
                    state.metrics.insert(agent_id.clone(), m);
                    if let Some(view) = state.server_view(&agent_id) {
                        let _ = state.dash_tx.send(DashboardEvent::Update { server: view });
                    }
                }
            }
            AgentMsg::FullReport(info) => {
                state.system_info.insert(agent_id.clone(), *info);
                if let Some(view) = state.server_view(&agent_id) {
                    let _ = state.dash_tx.send(DashboardEvent::Update { server: view });
                }
            }
            AgentMsg::DialResult { .. } => {
                // M6 接入落库
            }
            AgentMsg::Pong { .. } => {}
        }
    }

    // 5. 离线清理
    state.live.remove(&agent_id);
    state.persist_last_seen(&[agent_id.clone()]);
    tracing::info!(agent = %agent_id, "agent 离线");
    let _ = state.dash_tx.send(DashboardEvent::Offline { id: agent_id });
}

/// 校验 Hello；成功返回 agent_id。
async fn wait_hello(
    state: &AppState,
    socket: &mut WebSocket,
    challenge: &[u8; 32],
) -> Result<String, &'static str> {
    let deadline =
        tokio::time::timeout(Duration::from_secs(10), socket.recv()).await;
    let Ok(Some(Ok(Message::Binary(data)))) = deadline else {
        return Err("timeout-or-closed");
    };
    let msg: AgentMsg = postcard::from_bytes(&data).map_err(|_| "bad-encoding")?;
    let AgentMsg::Hello { agent_id, pubkey, timestamp, sig, proto_version } = msg else {
        return Err("expected-hello");
    };
    if proto_version != PROTOCOL_VERSION {
        return Err("version-mismatch");
    }

    // 时间窗校验（防重放）
    let now = crate::store::unix_now() as u64;
    if now.abs_diff(timestamp) > HANDSHAKE_TIMESTAMP_WINDOW_SECS {
        return Err("timestamp-window");
    }

    // 注册表校验：agent 必须预先注册，且公钥一致
    let expected = state
        .registry
        .get(&agent_id)
        .map(|k| *k)
        .ok_or("unknown-agent")?;
    if expected != pubkey {
        return Err("pubkey-mismatch");
    }

    // 验签
    let payload = handshake_signing_payload(challenge, &agent_id, timestamp);
    let vk = VerifyingKey::from_bytes(&pubkey).map_err(|_| "bad-pubkey")?;
    let signature = Signature::from_slice(&sig).map_err(|_| "bad-sig")?;
    vk.verify(&payload, &signature).map_err(|_| "bad-signature")?;

    Ok(agent_id)
}

pub async fn send_proto(socket: &mut WebSocket, msg: &ServerMsg) -> Result<(), axum::Error> {
    let bytes = postcard::to_allocvec(msg).expect("postcard 序列化不应失败");
    socket.send(Message::Binary(bytes.into())).await
}

/// 仪表盘 WS：连接即推全量快照，此后转发增量。
/// 需登录；viewer 按组过滤。
pub async fn ws_dashboard_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // 鉴权：从 Cookie 头提取 session
    let token = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split("; ").find_map(|c| {
                c.strip_prefix("zt_session=").map(str::to_string)
            })
        });
    let user = match token {
        Some(t) => state
            .store
            .with(|c| Ok(crate::auth::session_user(c, &t)))
            .ok()
            .flatten(),
        None => None,
    };
    let Some(user) = user else {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    };

    ws.on_upgrade(move |mut socket| async move {
        let allowed = |group_id: Option<i64>| -> bool {
            crate::api::server_visible_to(&user, group_id)
        };
        // 全量快照（按组过滤）
        let snapshot = json!({
            "type": "snapshot",
            "servers": state
                .server_views()
                .into_iter()
                .filter(|v| allowed(v.group_id))
                .collect::<Vec<_>>(),
        });
        if socket.send(Message::Text(snapshot.to_string().into())).await.is_err() {
            return;
        }
        // 增量订阅
        let mut rx: broadcast::Receiver<DashboardEvent> = state.dash_tx.subscribe();
        loop {
            tokio::select! {
                evt = rx.recv() => {
                    match evt {
                        Ok(e) => {
                            // 按用户组过滤增量事件
                            let visible = match &e {
                                DashboardEvent::Update { server } => allowed(server.group_id),
                                DashboardEvent::Offline { id } => state
                                    .server_view(id)
                                    .map(|v| allowed(v.group_id))
                                    .unwrap_or(false),
                            };
                            if !visible {
                                continue;
                            }
                            let text = serde_json::to_string(&e).expect("json 序列化");
                            if socket.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            // 消费滞后：重发快照（按组过滤）
                            let snapshot = json!({
                                "type": "snapshot",
                                "servers": state
                                    .server_views()
                                    .into_iter()
                                    .filter(|v| allowed(v.group_id))
                                    .collect::<Vec<_>>(),
                            });
                            if socket.send(Message::Text(snapshot.to_string().into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                msg = socket.recv() => {
                    // 客户端消息（心跳等）忽略；连接关闭则退出（防止空转）
                    if msg.is_none() {
                        break;
                    }
                }
            }
        }
    })
}
