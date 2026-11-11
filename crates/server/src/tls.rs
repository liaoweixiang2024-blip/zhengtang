//! 服务端 TLS：首启自动生成自签证书，SHA-256 指纹供 agent 固定。

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rcgen::{generate_simple_self_signed, CertifiedKey};
use sha2::{Digest, Sha256};

pub struct ServerCert {
    pub cert_pem: String,
    pub key_pem: String,
    /// 证书 DER 的 SHA-256 指纹（hex 小写）
    pub fingerprint_hex: String,
}

/// 若 data_dir 下已有 cert.pem/key.pem 则加载，否则生成并落盘。
pub fn ensure_cert(data_dir: &Path) -> Result<ServerCert> {
    let cert_path = data_dir.join("cert.pem");
    let key_path = data_dir.join("key.pem");

    if cert_path.exists() && key_path.exists() {
        let cert_pem = std::fs::read_to_string(&cert_path)?;
        let key_pem = std::fs::read_to_string(&key_path)?;
        let fingerprint = fingerprint_of_pem(&cert_pem)?;
        return Ok(ServerCert { cert_pem, key_pem, fingerprint_hex: fingerprint });
    }

    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["zhengtang".into()]).context("生成自签证书失败")?;
    let cert_pem = cert.pem();
    let key_pem = signing_key.serialize_pem();
    std::fs::write(&cert_path, &cert_pem)
        .with_context(|| format!("写入 {} 失败", cert_path.display()))?;
    std::fs::write(&key_path, &key_pem)
        .with_context(|| format!("写入 {} 失败", key_path.display()))?;

    // 权限收紧：私钥仅属主可读
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
    }

    let fingerprint = fingerprint_of_der(cert.der().as_ref());
    Ok(ServerCert { cert_pem, key_pem, fingerprint_hex: fingerprint })
}

fn fingerprint_of_der(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    data_encoding::HEXLOWER.encode(&digest)
}

fn fingerprint_of_pem(pem: &str) -> Result<String> {
    // 从 PEM 提取第一个 CERTIFICATE 块再求指纹
    let mut in_block = false;
    let mut b64 = String::new();
    for line in pem.lines() {
        if line.contains("BEGIN CERTIFICATE") {
            in_block = true;
        } else if line.contains("END CERTIFICATE") {
            break;
        } else if in_block {
            b64.push_str(line.trim());
        }
    }
    let der = data_encoding::BASE64
        .decode(b64.as_bytes())
        .context("解析证书 PEM 失败")?;
    Ok(fingerprint_of_der(&der))
}

/// 构建 rustls 服务端配置（ring provider 已在 main 安装为进程默认）。
pub fn build_tls_config(cert: &ServerCert) -> Result<Arc<rustls::ServerConfig>> {
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert.cert_pem.as_bytes())
            .collect::<std::result::Result<_, _>>()
            .context("解析证书链失败")?;
    let key = rustls_pemfile::private_key(&mut cert.key_pem.as_bytes())
        .context("解析私钥失败")?
        .context("私钥为空")?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("构建 TLS 配置失败")?;
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_stable_across_reload() {
        let dir = std::env::temp_dir().join(format!("zt-cert-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let c1 = ensure_cert(&dir).unwrap();
        let c2 = ensure_cert(&dir).unwrap();
        assert_eq!(c1.fingerprint_hex, c2.fingerprint_hex);
        assert!(!c1.fingerprint_hex.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
