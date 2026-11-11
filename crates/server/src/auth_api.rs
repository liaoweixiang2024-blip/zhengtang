//! 认证 + 管理 API：首启向导 / 登录(2FA) / 用户 / 组 / 服务器管理 / 审计。

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum_extra::extract::CookieJar;
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use rand::RngCore;
use serde::Deserialize;
use serde_json::json;

use crate::auth::{self, AuthedUser};
use crate::state::AppState;
use crate::store::unix_now;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // 首启向导（无用户时创建管理员）
        .route("/api/setup", post(setup))
        // 认证
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/auth/totp/setup", post(totp_setup))
        .route("/api/auth/totp/enable", post(totp_enable))
        // 管理（admin）
        .route("/api/admin/servers", get(admin_servers).post(add_server))
        .route("/api/admin/servers/{id}", patch(update_server).delete(delete_server))
        .route("/api/admin/groups", get(list_groups).post(add_group))
        .route("/api/admin/groups/{id}", delete(delete_group))
        .route("/api/admin/users", get(list_users).post(add_user))
        .route("/api/admin/users/{id}", delete(delete_user))
        .route("/api/admin/audit", get(list_audit))
        .route("/api/admin/fingerprint", get(get_fingerprint))
}

fn err(status: StatusCode, msg: &str) -> axum::response::Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

// ---------- 首启向导 ----------

#[derive(Deserialize)]
struct SetupBody {
    username: String,
    password: String,
}

async fn setup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<SetupBody>,
) -> impl IntoResponse {
    // 已有用户即禁止再 setup（无需登录态判断）
    let count = state
        .store
        .with(|c| Ok(c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))?))
        .unwrap_or(-1);
    if count != 0 {
        return err(StatusCode::FORBIDDEN, "已初始化，请直接登录");
    }
    if body.username.trim().len() < 3 || body.password.len() < 8 {
        return err(StatusCode::BAD_REQUEST, "用户名 ≥3 字符，密码 ≥8 字符");
    }
    let hash = match auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let ip = auth::client_ip(&headers, Some(peer));
    let token = state
        .store
        .with(|c| {
            c.execute(
                "INSERT INTO users (username, password_hash, role, created_at) VALUES (?1,?2,'admin',?3)",
                rusqlite::params![body.username.trim(), hash, unix_now()],
            )?;
            let uid: i64 = c.query_row(
                "SELECT id FROM users WHERE username = ?1",
                [&body.username.trim()],
                |r| r.get(0),
            )?;
            auth::audit(c, Some(uid), "setup", &format!("创建管理员 {uid}"), &ip);
            auth::create_session(c, uid, &ip)
        })
        .expect("初始化失败");
    (
        StatusCode::OK,
        [
            ("set-cookie", auth::session_cookie(&token, true)),
        ],
        Json(json!({ "ok": true, "role": "admin" })),
    )
        .into_response()
}

// ---------- 登录 ----------

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
    totp: Option<String>,
}

async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    _jar: CookieJar,
    Json(body): Json<LoginBody>,
) -> impl IntoResponse {
    let ip = auth::client_ip(&headers, Some(peer));
    let brute_key = format!("{ip}|{}", body.username);

    if state.brute.locked_for(&brute_key) > 0 {
        return err(StatusCode::TOO_MANY_REQUESTS, "尝试过于频繁，请稍后再试");
    }

    let row: Option<(i64, String, String, Option<String>, i64)> = state
        .store
        .with(|c| {
            Ok(c.query_row(
                "SELECT id, password_hash, role, totp_secret, totp_enabled FROM users WHERE username = ?1",
                [&body.username],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .ok())
        })
        .unwrap_or(None);

    let Some((uid, hash, role, totp_secret, totp_enabled)) = row else {
        // 用户不存在也计入失败（防用户名枚举的差异时序暂不处理）
        state.brute.record_fail(&brute_key);
        return err(StatusCode::UNAUTHORIZED, "用户名或密码错误");
    };

    if !auth::verify_password(&hash, &body.password) {
        state.brute.record_fail(&brute_key);
        auth::audit_state(&state, None, "login_fail", &body.username, &ip);
        return err(StatusCode::UNAUTHORIZED, "用户名或密码错误");
    }

    // 2FA
    if totp_enabled != 0 {
        let Some(secret) = totp_secret else {
            return err(StatusCode::INTERNAL_SERVER_ERROR, "2FA 状态异常");
        };
        let Some(code) = body.totp.as_deref() else {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "需要两步验证码", "totp_required": true })),
            )
                .into_response();
        };
        if !auth::verify_totp(&secret, code) {
            state.brute.record_fail(&brute_key);
            return err(StatusCode::UNAUTHORIZED, "两步验证码错误");
        }
    }

    state.brute.reset(&brute_key);
    let token = state
        .store
        .with(|c| {
            auth::audit(c, Some(uid), "login", "登录成功", &ip);
            auth::create_session(c, uid, &ip)
        })
        .expect("创建会话失败");
    (
        StatusCode::OK,
        [("set-cookie", auth::session_cookie(&token, true))],
        Json(json!({ "ok": true, "role": role })),
    )
        .into_response()
}

async fn logout(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get(auth::SESSION_COOKIE) {
        let token = cookie.value().to_string();
        let _ = state.store.with(|c| {
            c.execute("DELETE FROM sessions WHERE id = ?1", [&token])?;
            Ok(())
        });
    }
    (
        StatusCode::OK,
        [("set-cookie", auth::clear_cookie())],
        Json(json!({ "ok": true })),
    )
        .into_response()
}

async fn me(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    match auth::current_user(&state, &jar) {
        Some(u) => Json(json!({ "username": u.username, "role": u.role, "group_ids": u.group_ids })).into_response(),
        None => err(StatusCode::UNAUTHORIZED, "未登录"),
    }
}

// ---------- TOTP ----------

async fn totp_setup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
) -> impl IntoResponse {
    let Some(user) = auth::current_user(&state, &jar) else {
        return err(StatusCode::UNAUTHORIZED, "未登录");
    };
    let Ok((secret, url)) = auth::generate_totp_secret() else {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "生成失败");
    };
    // 暂存（未启用），enable 时校验通过才置 totp_enabled
    let _ = state.store.with(|c| {
        c.execute(
            "UPDATE users SET totp_secret = ?1 WHERE id = ?2",
            rusqlite::params![secret, user.id],
        )?;
        Ok(())
    });
    auth::audit_state(&state, Some(&user), "totp_setup", "", &auth::client_ip(&headers, Some(peer)));
    Json(json!({ "secret": secret, "url": url })).into_response()
}

#[derive(Deserialize)]
struct TotpEnableBody {
    code: String,
}

async fn totp_enable(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Json(body): Json<TotpEnableBody>,
) -> impl IntoResponse {
    let Some(user) = auth::current_user(&state, &jar) else {
        return err(StatusCode::UNAUTHORIZED, "未登录");
    };
    let secret: Option<String> = state
        .store
        .with(|c| {
            Ok(c.query_row("SELECT totp_secret FROM users WHERE id = ?1", [user.id], |r| r.get(0))
                .ok())
        })
        .unwrap_or(None);
    let Some(secret) = secret else {
        return err(StatusCode::BAD_REQUEST, "请先调用 setup 生成密钥");
    };
    if !auth::verify_totp(&secret, &body.code) {
        return err(StatusCode::BAD_REQUEST, "验证码错误");
    }
    let _ = state.store.with(|c| {
        c.execute("UPDATE users SET totp_enabled = 1 WHERE id = ?1", [user.id])?;
        Ok(())
    });
    Json(json!({ "ok": true })).into_response()
}

// ---------- 管理：服务器 ----------

fn require_admin(state: &AppState, jar: &CookieJar) -> Result<AuthedUser, axum::response::Response> {
    match auth::current_user(state, jar) {
        Some(u) if u.role == "admin" => Ok(u),
        Some(_) => Err(err(StatusCode::FORBIDDEN, "需要管理员权限")),
        None => Err(err(StatusCode::UNAUTHORIZED, "未登录")),
    }
}

async fn admin_servers(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    Json(json!({ "servers": state.server_views() })).into_response()
}

#[derive(Deserialize)]
struct AddServerBody {
    name: String,
    server_url: Option<String>,
}

/// API 版 add-server：生成密钥对，私钥一次性返回。
async fn add_server(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(body): Json<AddServerBody>,
) -> impl IntoResponse {
    let Ok(user) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    let agent_id = uuid::Uuid::new_v4().to_string();
    let mut secret = [0u8; 32];
    rand::rng().fill_bytes(&mut secret);
    let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
    let pubkey = signing.verifying_key().to_bytes();
    let secret_hex = data_encoding::HEXLOWER.encode(&secret);

    let _ = state.store.with(|c| crate::store::insert_server(c, &agent_id, &body.name, &pubkey));
    let _ = state.reload_registry();

    let fingerprint = crate::tls::FINGERPRINT
        .get()
        .map(|f| f.to_string())
        .unwrap_or_default();
    let base = body.server_url.unwrap_or_default().trim_end_matches('/').to_string();
    auth::audit_state(&state, Some(&user), "add_server", &body.name, &auth::client_ip(&headers, Some(peer)));

    Json(json!({
        "agent_id": agent_id,
        "key": secret_hex,
        "fingerprint": fingerprint,
        "server_url": base,
        "install_cmd": format!(
            "zhengtang-agent --server {base}/ws/agent --id {agent_id} --key {secret_hex} --fingerprint {fingerprint}"
        ),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct UpdateServerBody {
    name: Option<String>,
    group_id: Option<Option<i64>>,
    expires_at: Option<Option<i64>>,
    public_visible: Option<bool>,
    traffic_quota_bytes: Option<Option<i64>>,
}

async fn update_server(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<UpdateServerBody>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let _ = state.store.with(|c| {
        if let Some(n) = &body.name {
            c.execute("UPDATE servers SET name = ?1 WHERE id = ?2", rusqlite::params![n, id])?;
        }
        if let Some(Some(g)) = body.group_id {
            c.execute("UPDATE servers SET group_id = ?1 WHERE id = ?2", rusqlite::params![g, id])?;
        }
        if body.group_id == Some(None) {
            c.execute("UPDATE servers SET group_id = NULL WHERE id = ?1", [&id])?;
        }
        if let Some(e) = body.expires_at {
            let v: Option<i64> = e;
            c.execute("UPDATE servers SET expires_at = ?1 WHERE id = ?2", rusqlite::params![v, id])?;
        }
        if let Some(p) = body.public_visible {
            c.execute("UPDATE servers SET public_visible = ?1 WHERE id = ?2", rusqlite::params![p as i64, id])?;
        }
        if let Some(q) = body.traffic_quota_bytes {
            let v: Option<i64> = q;
            c.execute("UPDATE servers SET traffic_quota_bytes = ?1 WHERE id = ?2", rusqlite::params![v, id])?;
        }
        Ok(())
    });
    Json(json!({ "ok": true })).into_response()
}

async fn delete_server(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Ok(user) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    let _ = state.store.with(|c| {
        c.execute("DELETE FROM servers WHERE id = ?1", [&id])?;
        Ok(())
    });
    let _ = state.reload_registry();
    auth::audit_state(&state, Some(&user), "delete_server", &id, &auth::client_ip(&headers, Some(peer)));
    Json(json!({ "ok": true })).into_response()
}

// ---------- 管理：组 ----------

async fn list_groups(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let groups = state
        .store
        .with(|c| {
            let mut stmt = c.prepare("SELECT id, name FROM groups ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)? })))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    Json(json!({ "groups": groups })).into_response()
}

#[derive(Deserialize)]
struct AddGroupBody {
    name: String,
}

async fn add_group(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Json(body): Json<AddGroupBody>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let _ = state.store.with(|c| {
        c.execute("INSERT INTO groups (name) VALUES (?1)", [&body.name])?;
        Ok(())
    });
    Json(json!({ "ok": true })).into_response()
}

async fn delete_group(State(state): State<Arc<AppState>>, jar: CookieJar, Path(id): Path<i64>) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let _ = state.store.with(|c| {
        c.execute("DELETE FROM groups WHERE id = ?1", [id])?;
        Ok(())
    });
    Json(json!({ "ok": true })).into_response()
}

// ---------- 管理：用户 ----------

async fn list_users(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let users = state
        .store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, username, role, totp_enabled, created_at FROM users ORDER BY id",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "username": r.get::<_, String>(1)?,
                        "role": r.get::<_, String>(2)?,
                        "totp_enabled": r.get::<_, i64>(3)? != 0,
                        "created_at": r.get::<_, i64>(4)?,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    Json(json!({ "users": users })).into_response()
}

#[derive(Deserialize)]
struct AddUserBody {
    username: String,
    password: String,
    role: Option<String>,
    group_ids: Option<Vec<i64>>,
}

async fn add_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(body): Json<AddUserBody>,
) -> impl IntoResponse {
    let Ok(admin) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    if body.username.trim().len() < 3 || body.password.len() < 8 {
        return err(StatusCode::BAD_REQUEST, "用户名 ≥3 字符，密码 ≥8 字符");
    }
    let role = if body.role.as_deref() == Some("admin") { "admin" } else { "viewer" };
    let hash = match auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let res = state.store.with(|c| {
        c.execute(
            "INSERT INTO users (username, password_hash, role, created_at) VALUES (?1,?2,?3,?4)",
            rusqlite::params![body.username.trim(), hash, role, unix_now()],
        )?;
        let uid: i64 = c.query_row(
            "SELECT id FROM users WHERE username = ?1",
            [&body.username.trim()],
            |r| r.get(0),
        )?;
        for g in body.group_ids.unwrap_or_default() {
            c.execute(
                "INSERT OR IGNORE INTO user_groups (user_id, group_id) VALUES (?1,?2)",
                rusqlite::params![uid, g],
            )?;
        }
        Ok(())
    });
    if let Err(e) = res {
        if e.to_string().contains("UNIQUE") {
            return err(StatusCode::BAD_REQUEST, "用户名已存在");
        }
        return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    auth::audit_state(&state, Some(&admin), "add_user", &body.username, &auth::client_ip(&headers, Some(peer)));
    Json(json!({ "ok": true })).into_response()
}

async fn delete_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let Ok(admin) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    if admin.id == id {
        return err(StatusCode::BAD_REQUEST, "不能删除自己");
    }
    let _ = state.store.with(|c| {
        c.execute("DELETE FROM users WHERE id = ?1", [id])?;
        Ok(())
    });
    auth::audit_state(&state, Some(&admin), "delete_user", &id.to_string(), &auth::client_ip(&headers, Some(peer)));
    Json(json!({ "ok": true })).into_response()
}

// ---------- 审计 ----------

async fn list_audit(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let logs = state
        .store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT a.id, a.user_id, u.username, a.action, a.detail, a.ip, a.created_at
                 FROM audit_log a LEFT JOIN users u ON u.id = a.user_id
                 ORDER BY a.id DESC LIMIT 200",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "user": r.get::<_, Option<String>>(2)?,
                        "action": r.get::<_, String>(3)?,
                        "detail": r.get::<_, String>(4)?,
                        "ip": r.get::<_, Option<String>>(5)?,
                        "at": r.get::<_, i64>(6)?,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    Json(json!({ "logs": logs })).into_response()
}

// ---------- TLS 指纹（管理页展示安装命令用） ----------

async fn get_fingerprint(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let _ = state;
    let fp = crate::tls::FINGERPRINT.get().map(|f| f.to_string()).unwrap_or_default();
    Json(json!({ "fingerprint": fp })).into_response()
}
