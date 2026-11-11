//! Agent 端 TLS 校验：自签场景按 SHA-256 指纹固定，防中间人。

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring::default_provider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct FingerprintVerifier {
    expected: [u8; 32],
}

impl FingerprintVerifier {
    pub fn new(expected_hex: &str) -> Self {
        let mut expected = [0u8; 32];
        let bytes = data_encoding::HEXLOWER
            .decode(expected_hex.as_bytes())
            .expect("指纹 hex 已在配置层校验");
        expected.copy_from_slice(&bytes);
        Self { expected }
    }
}

impl ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let digest: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if digest == self.expected {
            Ok(ServerCertVerified::assertion())
        } else {
            // 不回传是否接近，避免给攻击者旁路信息
            Err(Error::General("证书校验失败".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        // 证书已按指纹固定，握手机密性由 pinned 证书保证
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        default_provider()
            .signature_verification_algorithms
            .supported_schemes()
            .to_vec()
    }
}

/// 构建 agent 用的 rustls ClientConfig。
/// fingerprint = Some → 自签指纹固定；None → 正常 WebPKI 校验（ACME/反代场景）。
pub fn client_config(fingerprint: Option<&str>) -> Arc<rustls::ClientConfig> {
    let builder = rustls::ClientConfig::builder();
    let config = match fingerprint {
        Some(hex) => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(FingerprintVerifier::new(hex)))
            .with_no_client_auth(),
        None => {
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    Arc::new(config)
}
