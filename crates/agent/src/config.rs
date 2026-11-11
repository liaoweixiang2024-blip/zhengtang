//! Agent 配置：CLI 参数优先，其次配置文件（TOML）。

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "zhengtang-agent", version, about = "zhengtang 探针 Agent（严格只读）")]
pub struct Args {
    /// 服务端 WS 地址，如 wss://host:8800/ws/agent
    #[arg(long)]
    pub server: Option<String>,

    /// agent_id（服务端注册时生成）
    #[arg(long)]
    pub id: Option<String>,

    /// Ed25519 私钥（64 位 hex，服务端注册时一次性下发）
    #[arg(long)]
    pub key: Option<String>,

    /// 服务端 TLS 证书 SHA-256 指纹（hex），用于防中间人。
    /// 未指定且 server 为 wss:// 时按普通 PKI 校验（适合 ACME/反代场景）。
    #[arg(long)]
    pub fingerprint: Option<String>,

    /// 配置文件路径（默认 /etc/zhengtang/agent.toml，存在即加载）
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FileConfig {
    pub server: Option<String>,
    pub id: Option<String>,
    pub key: Option<String>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub server: String,
    pub agent_id: String,
    pub secret: [u8; 32],
    /// Some(hex) = 指纹固定；None = 正常 PKI 校验
    pub fingerprint: Option<String>,
}

pub fn load(args: Args) -> Result<Config> {
    let path = args
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from("/etc/zhengtang/agent.toml"));
    let file_cfg: FileConfig = if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("解析配置失败: {}", path.display()))?
    } else {
        Default::default()
    };

    let server = args.server.or(file_cfg.server).context("缺少 --server 或配置文件 server 字段")?;
    let agent_id = args.id.or(file_cfg.id).context("缺少 --id 或配置文件 id 字段")?;
    let key_hex = args.key.or(file_cfg.key).context("缺少 --key 或配置文件 key 字段")?;
    let fingerprint = args.fingerprint.or(file_cfg.fingerprint);

    if !server.starts_with("ws://") && !server.starts_with("wss://") {
        bail!("server 地址必须以 ws:// 或 wss:// 开头");
    }
    // 明文连接 + 无指纹 = 凭据可被截获，直接拒绝（除非用户显式给 ws:// 且理解风险——仍要求指纹之外至少是 wss）
    if server.starts_with("ws://") {
        bail!("拒绝明文 ws:// 连接：请使用 wss://（自签场景配合 --fingerprint 指纹固定）");
    }

    let secret = data_encoding::HEXLOWER
        .decode(key_hex.trim().as_bytes())
        .context("私钥 hex 解码失败")?;
    if secret.len() != 32 {
        bail!("私钥应为 32 字节（64 hex 字符），当前 {} 字节", secret.len());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&secret);

    if let Some(fp) = fingerprint.as_deref() {
        let clean = fp.trim().replace(':', "").to_lowercase();
        if data_encoding::HEXLOWER.decode(clean.as_bytes()).ok().map(|b| b.len()) != Some(32) {
            bail!("--fingerprint 应为 32 字节的 SHA-256 hex");
        }
        return Ok(Config { server, agent_id, secret: arr, fingerprint: Some(clean) });
    }

    Ok(Config { server, agent_id, secret: arr, fingerprint: None })
}
