//! zhengtang-server：单二进制服务端。
//!
//! 默认在 :8800 起 HTTPS/WSS（自签证书 + 指纹固定）；`--http` 用于反代场景。

mod alert;
mod api;
mod auth;
mod auth_api;
mod embed;
mod gateway;
mod jobs;
mod state;
mod store;
mod tls;
mod update;
mod ppv2;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rand::RngCore;

use crate::state::AppState;

/// auth_api ↔ alert 模块的桥接（避免循环依赖细节外泄）。
pub fn alert_test_helper(id: i64, t: String, cfg: String) -> crate::alert::Channel {
    crate::alert::channel_from_row(id, t, cfg)
}

#[derive(Parser)]
#[command(name = "zhengtang-server", version, about = "zhengtang 探针服务端")]
struct Cli {
    /// 数据目录（数据库/证书所在）
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 启动服务（默认行为）
    Serve {
        /// 监听地址
        #[arg(long, default_value = "0.0.0.0:8800")]
        listen: String,
        /// 明文 HTTP 模式（配合反向代理终止 TLS 时使用）
        #[arg(long)]
        http: bool,
        /// agent 指标上报间隔（秒）
        #[arg(long, default_value_t = 3)]
        metrics_interval: u32,
    },
    /// 注册一台被监控服务器：生成密钥对并打印 Agent 安装参数
    AddServer {
        /// 服务器显示名称
        #[arg(long)]
        name: String,
        /// 服务器地址（用于生成安装命令展示，不影响协议）
        #[arg(long, default_value = "https://127.0.0.1:8800")]
        server_url: String,
    },
}

fn main() -> Result<()> {
    // 统一安装 ring crypto provider（全链路禁 openssl，musl 交叉编译友好）
    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.data_dir)
        .with_context(|| format!("创建数据目录失败: {}", cli.data_dir.display()))?;

    match cli.cmd {
        Some(Cmd::AddServer { name, server_url }) => add_server(&cli.data_dir, &name, &server_url),
        Some(Cmd::Serve { listen, http, metrics_interval }) => {
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
            runtime.block_on(serve(&cli.data_dir, &listen, http, metrics_interval))
        }
        None => {
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
            runtime.block_on(serve(&cli.data_dir, "0.0.0.0:8800", false, 3))
        }
    }
}

fn add_server(data_dir: &PathBuf, name: &str, server_url: &str) -> Result<()> {
    let db_path = data_dir.join("zhengtang.db");
    let store = Arc::new(store::Store::open(&db_path)?);

    // 生成 agent_id + Ed25519 密钥对（私钥只在此时显示一次，服务端仅存公钥）
    let agent_id = uuid::Uuid::new_v4().to_string();
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
    let pubkey = signing.verifying_key().to_bytes();
    let secret_hex = data_encoding::HEXLOWER.encode(&secret);

    store.with(|c| store::insert_server(c, &agent_id, name, &pubkey))?;

    let cert = tls::ensure_cert(data_dir)?;
    let base = server_url.trim_end_matches('/');

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  服务器已注册：{name:<42} ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  agent_id     : {agent_id}");
    println!("║  私钥(仅此一次): {secret_hex}");
    println!("║  TLS 指纹     : {}", cert.fingerprint_hex);
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();
    println!("Agent 启动参数：");
    println!("  zhengtang-agent \\\\");
    println!("    --server {base}/ws/agent \\\\");
    println!("    --id {agent_id} \\\\");
    println!("    --key {secret_hex} \\\\");
    println!("    --fingerprint {}", cert.fingerprint_hex);
    println!();
    println!("提示：私钥不会再显示，请立即保存；泄露可在管理页撤销重发。");
    Ok(())
}

async fn serve(data_dir: &PathBuf, listen: &str, http: bool, metrics_interval: u32) -> Result<()> {
    let db_path = data_dir.join("zhengtang.db");
    let store = Arc::new(store::Store::open(&db_path)?);
    let state = Arc::new(AppState::new(store, data_dir.clone(), metrics_interval)?);
    jobs::spawn(state.clone());
    alert::spawn(state.clone());

    let app = api::router()
        .merge(auth_api::router())
        .route("/ws/agent", axum::routing::get(gateway::ws_agent_handler))
        .route("/ws/dashboard", axum::routing::get(gateway::ws_dashboard_handler))
        .fallback(embed::static_handler)
        .with_state(state.clone());

    let addr: SocketAddr = listen.parse().with_context(|| format!("无效监听地址: {listen}"))?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let acceptor = if http {
        tracing::warn!(%addr, "以明文 HTTP 模式运行（请确保在可信反代之后）");
        None
    } else {
        let cert = tls::ensure_cert(data_dir)?;
        let tls_config = tls::build_tls_config(&cert)?;
        println!("zhengtang 服务端已启动: https://{addr}");
        println!("Agent 接入端点     : wss://{addr}/ws/agent");
        println!("TLS 证书指纹(SHA-256): {}", cert.fingerprint_hex);
        println!("（首启向导请在浏览器打开面板完成管理员创建）");
        Some(tokio_rustls::TlsAcceptor::from(tls_config))
    };

    // 手动 accept：TLS 与明文共用一条路径，且 accept 后先 peek PPv2 头——
    // frp 纯 TCP 穿透把源 IP 丢成 127.0.0.1，frpc 开 transport.proxyProtocolVersion=v2
    // 后转发流头部带真实来源地址；取出来替换 peer，last_ip/地区识别/审计 IP 全链路受益。
    // PPv2 头本身无鉴权：仅当 TCP 对端是回环/私网（frpc/反代与面板同机或同内网，
    // 本就是信任边界内）时才解析；公网直连的连接带 PPv2 头一律不采信——
    // 否则任意客户端可伪造源 IP 污染审计/last_ip/绕过登录爆破锁。
    // 普通直连 peek 一字节即放行，零开销
    loop {
        let (mut tcp, peer) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let app = app.clone();
        tokio::spawn(async move {
            let peer = if ppv2_trusted(&peer) {
                match ppv2::maybe_proxied(&mut tcp).await {
                    Ok(Some(real)) => real,
                    Ok(None) => peer,
                    Err(e) => {
                        tracing::debug!(%peer, error = %e, "连接异常（PPv2 探测阶段断开）");
                        return;
                    }
                }
            } else {
                peer
            };
            // TLS 流与裸 TCP 各自走泛型 serve_conn（多 trait 合不成一个 dyn 对象）
            match acceptor {
                Some(acceptor) => {
                    if let Ok(t) = acceptor.accept(tcp).await {
                        serve_conn(hyper_util::rt::TokioIo::new(t), &app, peer).await;
                    } else {
                        tracing::debug!(%peer, "TLS 握手失败");
                    }
                }
                None => {
                    serve_conn(hyper_util::rt::TokioIo::new(tcp), &app, peer).await;
                }
            };
        });
    }
}

/// PPv2 解析的信任边界：仅回环/私网对端（frpc、nginx 等本机/内网转发者）。
fn ppv2_trusted(peer: &SocketAddr) -> bool {
    peer.ip().is_loopback() || auth_api::is_private_ip(&peer.ip().to_string())
}

/// 单条连接：注入 ConnectInfo（含 PPv2 还原的真实源 IP）后交给 hyper
async fn serve_conn<I>(
    io: hyper_util::rt::TokioIo<I>,
    app: &axum::Router,
    peer: SocketAddr,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let app = app.clone();
    let service = hyper::service::service_fn(
        move |request: hyper::Request<hyper::body::Incoming>| {
            let mut app = app.clone();
            async move {
                use tower::Service;
                // 手动注入 ConnectInfo（等价 into_make_service_with_connect_info）
                let mut request = request.map(axum::body::Body::new);
                request.extensions_mut().insert(axum::extract::ConnectInfo(peer));
                app.call(request).await
            }
        },
    );
    if let Err(e) = hyper::server::conn::http1::Builder::new()
        .serve_connection(io, service)
        .with_upgrades()
        .await
    {
        tracing::debug!(%peer, error = %e, "HTTP 连接错误");
    }
}
// build trigger 1794464668
