//! WS 客户端：连接 → challenge 握手 → 周期上报 / 接收指令。

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use ed25519_dalek::Signer;
use futures_util::{SinkExt, StreamExt};
use proto::{
    handshake_signing_payload, AgentMsg, ServerMsg, HANDSHAKE_TIMESTAMP_WINDOW_SECS,
    MAX_MESSAGE_SIZE, PROTOCOL_VERSION,
};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;

use crate::collect::Collector;
use crate::config::Config;

struct AgentIdentity {
    agent_id: String,
    signing: ed25519_dalek::SigningKey,
}

/// 一次完整会话：连接、握手、运行直到断开。返回后由外层退避重连。
pub async fn run_session(cfg: &Config, collector: &mut Collector) -> Result<()> {
    let identity = AgentIdentity {
        agent_id: cfg.agent_id.clone(),
        signing: ed25519_dalek::SigningKey::from_bytes(&cfg.secret),
    };

    let mut request = cfg
        .server
        .as_str()
        .into_client_request()
        .context("无效的 server 地址")?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static("zhengtang-agent-v1"),
    );

    let connector = Connector::Rustls(crate::tls::client_config(cfg.fingerprint.as_deref()));
    let (mut ws, _resp) = tokio_tungstenite::connect_async_tls_with_config(
        request,
        None,
        false,
        Some(connector),
    )
    .await
    .context("连接服务端失败")?;

    // ---- 握手 ----
    let challenge = match ws.next().await {
        Some(Ok(Message::Binary(data))) => {
            let msg: ServerMsg = postcard::from_bytes(&data).context("解码 Challenge 失败")?;
            match msg {
                ServerMsg::Challenge { challenge } => challenge,
                other => bail!("期望 Challenge，收到 {other:?}"),
            }
        }
        other => bail!("等待 Challenge 失败: {other:?}"),
    };

    let timestamp = unix_now();
    let payload =
        handshake_signing_payload(&challenge, &identity.agent_id, timestamp);
    let sig = identity.signing.sign(&payload).to_bytes().to_vec();
    let hello = AgentMsg::Hello {
        agent_id: identity.agent_id.clone(),
        pubkey: identity.signing.verifying_key().to_bytes(),
        timestamp,
        sig,
        proto_version: PROTOCOL_VERSION,
    };
    send(&mut ws, &hello).await?;

    // 上报间隔由 HelloOk 下发
    let interval_secs: u32;
    match ws.next().await {
        Some(Ok(Message::Binary(data))) => {
            match postcard::from_bytes::<ServerMsg>(&data).context("解码握手应答失败")? {
                ServerMsg::HelloOk { metrics_interval_secs, server_time } => {
                    // 时钟偏移提示（不影响连接）
                    let drift = (server_time as i64 - unix_now() as i64).unsigned_abs();
                    if drift > HANDSHAKE_TIMESTAMP_WINDOW_SECS {
                        tracing::warn!(drift_secs = drift, "本机时钟与服务端偏差较大，请校时（NTP）");
                    }
                    interval_secs = metrics_interval_secs.clamp(1, 60);
                }
                ServerMsg::HelloErr => bail!("服务端拒绝握手（常见原因：面板上删除重加过该服务器未重装；本机时间偏差超过 60 秒——跑 timedatectl set-ntp true 校时后重启 agent）"),
                other => bail!("期望握手应答，收到 {other:?}"),
            }
        }
        other => bail!("等待握手应答失败: {other:?}"),
    }
    tracing::info!(server = %cfg.server, interval = interval_secs, "已上线，开始上报");

    // ---- 运行：指标通道 + 接收循环 + 拨测调度 ----
    let (tx, mut rx) = mpsc::channel::<AgentMsg>(16);
    let (dial_results_tx, mut dial_results_rx) = mpsc::unbounded_channel::<AgentMsg>();
    let scheduler = crate::dial::DialScheduler::new(dial_results_tx);
    let dial_tasks = scheduler.tasks.clone();
    let dial_handle = tokio::spawn(scheduler.run());

    // 采集上报任务
    let report_tx = tx.clone();
    let collect_handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs as u64));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // 首个 tick 立即返回，跳过
        // 立即上报一次（含 FullReport 由主循环处理）
        loop {
            ticker.tick().await;
            // 通道关闭 = 会话结束
            if report_tx.is_closed() {
                break;
            }
            // 采集通过消息请求主循环执行（Collector 非线程安全，归主循环所有）
            if report_tx.send(AgentMsg::Pong { nonce: COLLECT_NOW }).await.is_err() {
                break;
            }
        }
    });

    let mut full_report_due = true; // 上线先报一次
    let mut last_full_report = tokio::time::Instant::now();
    let session: Result<()> = loop {
        tokio::select! {
            out = ws.next() => {
                match out {
                    Some(Ok(Message::Binary(data))) => {
                        if data.len() > MAX_MESSAGE_SIZE { break Err(anyhow!("服务端消息超限")); }
                        match postcard::from_bytes::<ServerMsg>(&data) {
                            Ok(ServerMsg::Ping { nonce }) => {
                                if let Err(e) = send(&mut ws, &AgentMsg::Pong { nonce }).await {
                                    break Err(anyhow!(e));
                                }
                            }
                            Ok(ServerMsg::ProcessRules(rules)) => collector.set_process_rules(rules),
                            Ok(ServerMsg::DialTasks(tasks)) => {
                                tracing::info!(count = tasks.len(), "收到拨测任务");
                                *dial_tasks.write().await = tasks;
                            }
                            Ok(ServerMsg::RequestFullReport) => full_report_due = true,
                            Ok(other) => tracing::debug!(?other, "忽略服务端消息"),
                            Err(e) => tracing::warn!(%e, "无法解码服务端消息"),
                        }
                    }
                    Some(Ok(_)) => {} // 非 Binary 帧忽略
                    Some(Err(e)) => break Err(anyhow!("连接错误: {e}")),
                    None => break Err(anyhow!("连接关闭")),
                }
            }
            r = dial_results_rx.recv() => {
                // 拨测结果直接透传（通道关闭不该发生）
                let Some(msg) = r else { break Err(anyhow!("拨测通道关闭")) };
                if let Err(e) = send(&mut ws, &msg).await {
                    break Err(anyhow!(e));
                }
            }
            msg = rx.recv() => {
                let Some(msg) = msg else { break Err(anyhow!("采集任务结束")) };
                let outgoing: AgentMsg = match msg {
                    AgentMsg::Pong { nonce: COLLECT_NOW } => {
                        AgentMsg::Metrics(collector.snapshot())
                    }
                    m => m,
                };
                if let Err(e) = send(&mut ws, &outgoing).await {
                    break Err(anyhow!(e));
                }
                // 周期性 FullReport（5 分钟）
                if full_report_due || last_full_report.elapsed() >= Duration::from_secs(300) {
                    if let Err(e) = send(&mut ws, &AgentMsg::FullReport(Box::new(collector.full_report()))).await {
                        break Err(anyhow!(e));
                    }
                    full_report_due = false;
                    last_full_report = tokio::time::Instant::now();
                }
            }
        }
    };
    collect_handle.abort();
    dial_handle.abort();
    session
}

/// 特殊 nonce：触发一次采集（内部协议，不上报 Pong 本身）。
const COLLECT_NOW: u64 = u64::MAX;

async fn send(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    msg: &AgentMsg,
) -> Result<()> {
    let bytes = postcard::to_allocvec(msg).expect("postcard 序列化不应失败");
    ws.send(Message::Binary(bytes.into()))
        .await
        .context("发送失败")?;
    Ok(())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
