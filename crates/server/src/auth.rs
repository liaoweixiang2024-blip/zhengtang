//! 用户认证：argon2id 密码 + DB session + TOTP 2FA + 登录防爆破 + 审计。

use std::sync::Arc;

use anyhow::Result;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum_extra::extract::CookieJar;
use dashmap::DashMap;
use rusqlite::Connection;

use crate::state::AppState;
use crate::store::unix_now;

pub const SESSION_COOKIE: &str = "zt_session";
const SESSION_TTL_SECS: i64 = 7 * 86400;

#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub id: i64,
    pub username: String,
    pub role: String, // "admin" | "viewer"
    pub group_ids: Vec<i64>,
}

// ---------- 密码 ----------

pub fn hash_password(password: &str) -> Result<String> {
    // salt 用自身随机源生成（16B → base64），不依赖 password-hash 的 rand_core feature
    let mut b = [0u8; 16];
    use rand::RngCore;
    rand::rng().fill_bytes(&mut b);
    let b64 = data_encoding::BASE64_NOPAD.encode(&b);
    let salt = SaltString::from_b64(&b64).map_err(|e| anyhow::anyhow!("salt 生成失败: {e}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("密码哈希失败: {e}"))?
        .to_string())
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

// ---------- session ----------

pub fn create_session(conn: &Connection, user_id: i64, ip: &str) -> Result<String> {
    let token = data_encoding::HEXLOWER.encode(&rand32());
    conn.execute(
        "INSERT INTO sessions (id, user_id, expires_at, ip) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![token, user_id, unix_now() + SESSION_TTL_SECS, ip],
    )?;
    // 顺手清理过期 session
    let _ = conn.execute("DELETE FROM sessions WHERE expires_at < ?1", [unix_now()]);
    Ok(token)
}

pub fn session_user(conn: &Connection, token: &str) -> Option<AuthedUser> {
    let row: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT u.id, u.username, u.role FROM sessions s
             JOIN users u ON u.id = s.user_id
             WHERE s.id = ?1 AND s.expires_at > ?2",
            rusqlite::params![token, unix_now()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let (id, username, role) = row?;
    let group_ids = conn
        .prepare("SELECT group_id FROM user_groups WHERE user_id = ?1")
        .and_then(|mut s| {
            s.query_map([id], |r| r.get::<_, i64>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()
        })
        .unwrap_or_default();
    Some(AuthedUser { id, username, role, group_ids })
}

/// 从 CookieJar 提取当前用户（None = 未登录）。
pub fn current_user(state: &AppState, jar: &CookieJar) -> Option<AuthedUser> {
    let token = jar.get(SESSION_COOKIE)?.value();
    state.store.with(|c| Ok(session_user(c, token))).ok().flatten()
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    // HttpOnly + SameSite=Lax；TLS 模式加 Secure
    format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECS}{}", if secure { "; Secure" } else { "" })
}

pub fn clear_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

fn rand32() -> [u8; 32] {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    b
}

// ---------- TOTP ----------

pub fn generate_totp_secret() -> Result<(String, String)> {
    use totp_rs::{Algorithm, Secret, TOTP};
    let secret = Secret::generate_secret(); // 默认 32 字节
    let bytes = secret.to_bytes().map_err(|e| anyhow::anyhow!("{e}"))?;
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        bytes,
        Some("zhengtang".to_string()),
        "zhengtang".to_string(),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let url = totp.get_url();
    let encoded = secret.to_encoded().to_string();
    Ok((encoded, url))
}

pub fn verify_totp(secret_base32: &str, code: &str) -> bool {
    use totp_rs::{Algorithm, Secret, TOTP};
    match Secret::Encoded(secret_base32.to_string()).to_bytes() {
        Ok(bytes) => {
            match TOTP::new(
                Algorithm::SHA1,
                6,
                1,
                30,
                bytes,
                Some("zhengtang".to_string()),
                "zhengtang".to_string(),
            ) {
                Ok(t) => t.check_current(code).unwrap_or(false),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

// ---------- 防爆破 ----------

#[derive(Default)]
pub struct BruteGuard {
    // key: "ip|username" → (失败次数, 解锁时间戳)
    map: DashMap<String, (u32, i64)>,
}

impl BruteGuard {
    /// 距解锁还需秒数；0 = 允许尝试
    pub fn locked_for(&self, key: &str) -> i64 {
        if let Some(entry) = self.map.get(key) {
            let (.., until) = *entry;
            if until > unix_now() {
                return until - unix_now();
            }
        }
        0
    }

    pub fn record_fail(&self, key: &str) {
        let (count, _) = self.map.get(key).map(|v| *v).unwrap_or((0, 0));
        let count = count + 1;
        // 指数退避：2^n 秒，5 次起锁，上限 1 小时
        let lock = if count >= 5 {
            (2u64.saturating_pow(count - 5).min(3600)) as i64
        } else {
            0
        };
        self.map.insert(key.to_string(), (count, unix_now() + lock));
    }

    pub fn reset(&self, key: &str) {
        self.map.remove(key);
    }
}

// ---------- 审计 ----------

pub fn audit(conn: &Connection, user_id: Option<i64>, action: &str, detail: &str, ip: &str) {
    let _ = conn.execute(
        "INSERT INTO audit_log (user_id, action, detail, ip, created_at) VALUES (?1,?2,?3,?4,?5)",
        rusqlite::params![user_id, action, detail, ip, unix_now()],
    );
}

/// 从请求头取客户端 IP（反代场景信任 X-Forwarded-For 首段）。
pub fn client_ip(headers: &axum::http::HeaderMap, peer: Option<std::net::SocketAddr>) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| peer.map(|p| p.ip().to_string()))
        .unwrap_or_else(|| "unknown".into())
}

/// AppState 上的便捷封装。
pub fn audit_state(state: &Arc<AppState>, user: Option<&AuthedUser>, action: &str, detail: &str, ip: &str) {
    let _ = state.store.with(|c| {
        audit(c, user.map(|u| u.id), action, detail, ip);
        Ok(())
    });
}
