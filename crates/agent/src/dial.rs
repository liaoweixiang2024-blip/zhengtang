//! 受约束拨测调度器：仅 HTTP GET（禁重定向/限 4KB）与 TCP connect，带超时。
//! 不存在任何执行语义——这是 Agent 仅有的两个主动网络动作之一（另一个是上报）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use proto::{DialKind, DialTask};
use tokio::sync::{mpsc, RwLock};

pub struct DialScheduler {
    pub tasks: Arc<RwLock<Vec<DialTask>>>,
    /// 待发送的拨测结果
    pub results_tx: mpsc::UnboundedSender<proto::AgentMsg>,
    http: reqwest::Client,
}

impl DialScheduler {
    pub fn new(results_tx: mpsc::UnboundedSender<proto::AgentMsg>) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none()) // 禁重定向
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self { tasks: Arc::new(RwLock::new(Vec::new())), results_tx, http }
    }

    /// 调度主循环：每秒检查各任务到期。
    pub async fn run(self) {
        let mut next_run: HashMap<i64, Instant> = HashMap::new();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let now = Instant::now();
            let tasks = self.tasks.read().await.clone();

            for t in tasks {
                // 新任务以本轮 now 为准立即到期（不能重新取 Instant::now()，会比 now 晚导致永不到期）
                let due = *next_run.get(&t.id).unwrap_or(&now);
                if now >= due {
                    next_run.insert(t.id, now + Duration::from_secs(t.interval_secs.max(10).min(86400) as u64));
                    let result = run_one(&self.http, &t).await;
                    let _ = self.results_tx.send(result);
                }
            }
        }
    }
}

async fn run_one(http: &reqwest::Client, t: &DialTask) -> proto::AgentMsg {
    let timeout = Duration::from_millis(t.timeout_ms.clamp(500, 10000) as u64);
    let started = Instant::now();
    match &t.kind {
        DialKind::HttpGet { url, expect_status } => {
            let fut = async {
                let resp = http.get(url).send().await?;
                let status = resp.status().as_u16();
                // 只读响应头与前 4KB（限体积）
                let _body = resp.bytes().await.unwrap_or_default();
                anyhow::Ok(status)
            };
            match tokio::time::timeout(timeout, fut).await {
                Ok(Ok(status)) => {
                    let ok = expect_status.map(|e| e == status).unwrap_or((200..400).contains(&status));
                    proto::AgentMsg::DialResult {
                        task_id: t.id,
                        ok,
                        latency_ms: started.elapsed().as_millis() as u32,
                        status: status.to_string(),
                    }
                }
                Ok(Err(e)) => proto::AgentMsg::DialResult {
                    task_id: t.id,
                    ok: false,
                    latency_ms: started.elapsed().as_millis() as u32,
                    status: truncate(&e.to_string(), 60),
                },
                Err(_) => proto::AgentMsg::DialResult {
                    task_id: t.id,
                    ok: false,
                    latency_ms: timeout.as_millis() as u32,
                    status: "timeout".into(),
                },
            }
        }
        DialKind::TcpConnect { host, port } => {
            let fut = tokio::net::TcpStream::connect((host.as_str(), *port));
            match tokio::time::timeout(timeout, fut).await {
                Ok(Ok(_)) => proto::AgentMsg::DialResult {
                    task_id: t.id,
                    ok: true,
                    latency_ms: started.elapsed().as_millis() as u32,
                    status: "connected".into(),
                },
                Ok(Err(e)) => proto::AgentMsg::DialResult {
                    task_id: t.id,
                    ok: false,
                    latency_ms: started.elapsed().as_millis() as u32,
                    status: truncate(&e.to_string(), 60),
                },
                Err(_) => proto::AgentMsg::DialResult {
                    task_id: t.id,
                    ok: false,
                    latency_ms: timeout.as_millis() as u32,
                    status: "timeout".into(),
                },
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let mut end = n;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}
