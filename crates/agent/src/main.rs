//! zhengtang-agent：严格只读探针。
//!
//! 只做三件事：连接服务端、采集本机指标、执行受约束拨测。
//! 协议层不存在任何命令执行原语（见 proto::ServerMsg）。

mod client;
mod collect;
mod config;
mod tls;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = config::Args::parse();
    let cfg = config::load(args)?;

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(run(cfg))
}

async fn run(cfg: config::Config) -> Result<()> {
    let mut collector = collect::Collector::new()?;
    let mut backoff_secs = 1u64;
    loop {
        match client::run_session(&cfg, &mut collector).await {
            Ok(()) => {
                tracing::info!("会话正常结束");
                backoff_secs = 1;
            }
            Err(e) => {
                tracing::warn!(error = %e, backoff_secs, "会话断开，稍后重连");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(60); // 指数退避，上限 60s
    }
}
