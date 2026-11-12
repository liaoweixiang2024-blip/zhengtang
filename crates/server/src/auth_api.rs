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
        .route("/api/admin/settings", get(get_settings).put(put_settings))
        // 告警
        .route("/api/alerts/events", get(alert_events))
        .route("/api/admin/channels", get(list_channels).post(add_channel))
        .route("/api/admin/channels/{id}", delete(delete_channel))
        .route("/api/admin/channels/{id}/test", post(test_channel))
        // 拨测任务
        .route("/api/admin/servers/{id}/dial-tasks", get(list_dial_tasks).post(add_dial_task))
        .route("/api/admin/dial-tasks/{task_id}", delete(delete_dial_task))
        // 进程监控规则
        .route("/api/admin/servers/{id}/process-rules", get(list_process_rules).post(add_process_rule))
        .route("/api/admin/process-rules/{rule_id}", delete(delete_process_rule))
        // 拨测结果（详情页，登录用户）
        .route("/api/servers/{id}/dial-results", get(dial_results))
        // 分组名（仪表盘分组标签用，登录用户；viewer 只见其授权组）
        .route("/api/groups", get(my_groups))
}

/// 登录用户可见的分组（admin 全量；viewer 按授权过滤）
async fn my_groups(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    let Some(user) = crate::auth::current_user(&state, &jar) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "未登录" })),
        )
            .into_response();
    };
    let groups = state
        .store
        .with(|c| {
            let mut stmt = c.prepare("SELECT id, name FROM groups ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    let visible: Vec<_> = groups
        .into_iter()
        .filter(|(id, _)| user.role == "admin" || user.group_ids.contains(id))
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    Json(json!({ "groups": visible })).into_response()
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

/// 双层 Option：区分「字段缺失」(None=不改) 与 「JSON null」(Some(None)=清空)。
/// serde 默认把 null 也读成 None，导致所有"清空"操作静默失效。
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    // null → Some(None)（清空）；有值 → Some(Some(v))（更新）
    Option::<T>::deserialize(de).map(Some)
}

#[derive(Deserialize)]
struct UpdateServerBody {
    name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    group_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    expires_at: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    purchased_at: Option<Option<i64>>,
    /// 1-28；None=自然月
    #[serde(default, deserialize_with = "double_option")]
    traffic_reset_day: Option<Option<i64>>,
    /// 付费周期月数：1/3/12/36/60；0=一次性；-1=免费
    #[serde(default, deserialize_with = "double_option")]
    pay_cycle_months: Option<Option<i64>>,
    /// 每期费用（≥0）
    #[serde(default, deserialize_with = "double_option")]
    price_per_cycle: Option<Option<f64>>,
    /// CNY/USD/EUR/JPY/HKD/GBP
    #[serde(default, deserialize_with = "double_option")]
    currency: Option<Option<String>>,
    /// 地区徽章（如 '🇭🇰 香港'，≤24 字符）
    #[serde(default, deserialize_with = "double_option")]
    region: Option<Option<String>>,
    public_visible: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    traffic_quota_bytes: Option<Option<i64>>,
}

/// 付费周期与货币的合法值（0=一次性、-1=免费）
const PAY_CYCLE_MONTHS: [i64; 7] = [1, 3, 12, 36, 60, 0, -1];
const CURRENCIES: [&str; 6] = ["CNY", "USD", "EUR", "JPY", "HKD", "GBP"];

async fn update_server(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<UpdateServerBody>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let result = state.store.with(|c| {
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
        if let Some(p) = body.purchased_at {
            let v: Option<i64> = p;
            c.execute("UPDATE servers SET purchased_at = ?1 WHERE id = ?2", rusqlite::params![v, id])?;
        }
        if let Some(rd) = body.traffic_reset_day {
            // 合法值：1-28 或 NULL（自然月）
            if let Some(d) = rd {
                if !(1..=28).contains(&d) {
                    anyhow::bail!("流量重置日必须是 1-28");
                }
            }
            c.execute("UPDATE servers SET traffic_reset_day = ?1 WHERE id = ?2", rusqlite::params![rd, id])?;
        }
        if let Some(q) = body.traffic_quota_bytes {
            let v: Option<i64> = q;
            c.execute("UPDATE servers SET traffic_quota_bytes = ?1 WHERE id = ?2", rusqlite::params![v, id])?;
        }
        if let Some(m) = body.pay_cycle_months {
            if let Some(v) = m {
                if !PAY_CYCLE_MONTHS.contains(&v) {
                    anyhow::bail!("付费周期必须是 1/3/12/36/60 月、一次性或免费");
                }
            }
            c.execute("UPDATE servers SET pay_cycle_months = ?1 WHERE id = ?2", rusqlite::params![m, id])?;
            // 免费 = 无费用无货币（覆盖旧值，避免残留 $99/免费 的矛盾数据）
            if m == Some(-1) {
                c.execute("UPDATE servers SET price_per_cycle = NULL, currency = NULL WHERE id = ?1", [&id])?;
            }
        }
        if let Some(p) = body.price_per_cycle {
            if let Some(v) = p {
                if !v.is_finite() || v < 0.0 {
                    anyhow::bail!("费用必须是不小于 0 的数字");
                }
            }
            c.execute("UPDATE servers SET price_per_cycle = ?1 WHERE id = ?2", rusqlite::params![p, id])?;
        }
        if let Some(cur) = &body.currency {
            if let Some(v) = cur {
                if !CURRENCIES.contains(&v.as_str()) {
                    anyhow::bail!("货币必须是 CNY/USD/EUR/JPY/HKD/GBP");
                }
            }
            c.execute("UPDATE servers SET currency = ?1 WHERE id = ?2", rusqlite::params![cur, id])?;
        }
        if let Some(region) = &body.region {
            if let Some(v) = region {
                if v.chars().count() > 24 {
                    anyhow::bail!("地区徽章过长（≤24 字符）");
                }
            }
            c.execute("UPDATE servers SET region = ?1 WHERE id = ?2", rusqlite::params![region, id])?;
        }
        Ok(())
    });
    match result {
        Ok(()) => {
            // 已连接的仪表盘即时刷新（否则要等重连拿新快照）
            if let Some(v) = state.server_view(&id) {
                let _ = state
                    .dash_tx
                    .send(crate::state::DashboardEvent::Update { server: v });
            }
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
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

// ---------- 告警 ----------

/// 最近告警事件（登录可见；viewer 只看其组）。
async fn alert_events(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> impl IntoResponse {
    let Some(user) = auth::current_user(&state, &jar) else {
        return err(StatusCode::UNAUTHORIZED, "未登录");
    };
    let events = state
        .store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT e.id, e.server_id, s.name, s.group_id, e.fired_at, e.resolved_at, e.summary
                 FROM alert_events e JOIN servers s ON s.id = e.server_id
                 ORDER BY e.id DESC LIMIT 200",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        r.get::<_, String>(6)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    let events: Vec<_> = events
        .into_iter()
        .filter(|(_, _, _, gid, ..)| crate::api::server_visible_to(&user, *gid))
        .map(|(id, sid, name, _, fired, resolved, summary)| {
            json!({ "id": id, "server": name, "server_id": sid, "fired_at": fired, "resolved_at": resolved, "summary": summary })
        })
        .collect();
    Json(json!({ "events": events })).into_response()
}

async fn list_channels(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let channels = state
        .store
        .with(|c| {
            let mut stmt = c.prepare("SELECT id, type, name FROM notify_channels ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(json!({ "id": r.get::<_, i64>(0)?, "type": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)? }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    Json(json!({ "channels": channels })).into_response()
}

#[derive(Deserialize)]
struct AddChannelBody {
    #[serde(rename = "type")]
    type_: String,
    name: String,
    config: serde_json::Value,
}

async fn add_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(body): Json<AddChannelBody>,
) -> impl IntoResponse {
    let Ok(admin) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    let _ = state.store.with(|c| {
        c.execute(
            "INSERT INTO notify_channels (type, name, config) VALUES (?1,?2,?3)",
            rusqlite::params![body.type_, body.name, body.config.to_string()],
        )?;
        Ok(())
    });
    auth::audit_state(&state, Some(&admin), "add_channel", &body.name, &auth::client_ip(&headers, Some(peer)));
    Json(json!({ "ok": true })).into_response()
}

async fn delete_channel(State(state): State<Arc<AppState>>, jar: CookieJar, Path(id): Path<i64>) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let _ = state.store.with(|c| {
        c.execute("DELETE FROM notify_channels WHERE id = ?1", [id])?;
        Ok(())
    });
    Json(json!({ "ok": true })).into_response()
}

/// 测试发送（不落通知历史之外的逻辑）。
async fn test_channel(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let ch = state
        .store
        .with(|c| {
            let row = c
                .query_row(
                    "SELECT id, type, config FROM notify_channels WHERE id = ?1",
                    [id],
                    |r| {
                        Ok(crate::alert_test_helper(id, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
                    },
                )
                .ok();
            Ok(row)
        })
        .unwrap_or(None);
    let Some(ch) = ch else {
        return err(StatusCode::NOT_FOUND, "渠道不存在");
    };
    let ok = crate::alert::send_one(&ch, "zhengtang 测试通知：渠道配置正常", "zhengtang 测试").await;
    if ok {
        Json(json!({ "ok": true })).into_response()
    } else {
        err(StatusCode::BAD_GATEWAY, "发送失败，查看服务端日志")
    }
}


// ---------- 拨测 ----------

async fn list_dial_tasks(State(state): State<Arc<AppState>>, jar: CookieJar, Path(id): Path<String>) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let tasks = state
        .store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, kind, target, interval_secs, enabled FROM dial_tasks WHERE server_id = ?1 ORDER BY id",
            )?;
            let rows = stmt
                .query_map([&id], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "kind": r.get::<_, String>(1)?,
                        "target": r.get::<_, String>(2)?,
                        "interval_secs": r.get::<_, i64>(3)?,
                        "enabled": r.get::<_, i64>(4)? != 0,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    Json(json!({ "tasks": tasks })).into_response()
}

#[derive(Deserialize)]
struct AddDialTaskBody {
    /// "http" | "tcp"
    kind: String,
    /// http: 完整 URL；tcp: host:port
    target: String,
    #[serde(default = "default_interval")]
    interval_secs: i64,
}

fn default_interval() -> i64 {
    60
}

async fn add_dial_task(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<AddDialTaskBody>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    // SSRF 防护：仅 http/https 的 URL、或 host:port；长度限制；协议白名单
    if body.target.len() > 512 {
        return err(StatusCode::BAD_REQUEST, "目标过长");
    }
    match body.kind.as_str() {
        "http" => {
            if !body.target.starts_with("http://") && !body.target.starts_with("https://") {
                return err(StatusCode::BAD_REQUEST, "http 任务目标必须是 http(s):// URL");
            }
        }
        "tcp" => {
            if body.target.contains("://") || !body.target.contains(':') {
                return err(StatusCode::BAD_REQUEST, "tcp 任务目标必须是 host:port");
            }
        }
        _ => return err(StatusCode::BAD_REQUEST, "kind 必须是 http 或 tcp"),
    }
    let interval = body.interval_secs.clamp(10, 86400);
    let _ = state.store.with(|c| {
        c.execute(
            "INSERT INTO dial_tasks (server_id, kind, target, interval_secs) VALUES (?1,?2,?3,?4)",
            rusqlite::params![id, body.kind, body.target, interval],
        )?;
        Ok(())
    });
    state.push_dial_tasks(&id);
    Json(json!({ "ok": true })).into_response()
}

async fn delete_dial_task(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(task_id): Path<i64>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    // 找回 server_id 用于推送更新
    let sid: Option<String> = state
        .store
        .with(|c| {
            Ok(c.query_row("SELECT server_id FROM dial_tasks WHERE id = ?1", [task_id], |r| r.get(0)).ok())
        })
        .unwrap_or(None);
    let _ = state.store.with(|c| {
        c.execute("DELETE FROM dial_tasks WHERE id = ?1", [task_id])?;
        Ok(())
    });
    if let Some(sid) = sid {
        state.push_dial_tasks(&sid);
    }
    Json(json!({ "ok": true })).into_response()
}

/// 最近拨测结果（详情页展示，登录用户按组过滤）。
async fn dial_results(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(user) = auth::current_user(&state, &jar) else {
        return err(StatusCode::UNAUTHORIZED, "未登录");
    };
    let group_id: Option<Option<i64>> = state
        .store
        .with(|c| {
            Ok(c.query_row("SELECT group_id FROM servers WHERE id = ?1", [&id], |r| r.get(0)).ok())
        })
        .unwrap_or(None);
    let Some(group_id) = group_id else {
        return err(StatusCode::NOT_FOUND, "服务器不存在");
    };
    if !crate::api::server_visible_to(&user, group_id) {
        return err(StatusCode::FORBIDDEN, "无权访问");
    }
    // 每任务最近 20 条
    let results = state
        .store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT t.id, t.kind, t.target, r.ok, r.latency_ms, r.status, r.ts
                 FROM dial_tasks t
                 LEFT JOIN (
                     SELECT task_id, ok, latency_ms, status, ts,
                            ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY ts DESC) rn
                     FROM dial_results
                 ) r ON r.task_id = t.id AND r.rn <= 20
                 WHERE t.server_id = ?1
                 ORDER BY t.id, r.ts DESC",
            )?;
            let rows = stmt
                .query_map([&id], |r| {
                    Ok(json!({
                        "task_id": r.get::<_, i64>(0)?,
                        "kind": r.get::<_, String>(1)?,
                        "target": r.get::<_, String>(2)?,
                        "ok": r.get::<_, Option<i64>>(3)?.map(|v| v != 0),
                        "latency_ms": r.get::<_, Option<i64>>(4)?,
                        "status": r.get::<_, Option<String>>(5)?,
                        "ts": r.get::<_, Option<i64>>(6)?,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    Json(json!({ "results": results })).into_response()
}


// ---------- 进程监控 ----------

async fn list_process_rules(State(state): State<Arc<AppState>>, jar: CookieJar, Path(id): Path<String>) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let rules = state
        .store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, exact, COALESCE(display_name, name) FROM process_rules WHERE server_id = ?1 ORDER BY id",
            )?;
            let rows = stmt
                .query_map([&id], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "exact": r.get::<_, i64>(2)? != 0,
                        "display_name": r.get::<_, String>(3)?,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap_or_default();
    Json(json!({ "rules": rules })).into_response()
}

#[derive(Deserialize)]
struct AddProcessRuleBody {
    name: String,
    #[serde(default = "default_true")]
    exact: bool,
    display_name: Option<String>,
}

fn default_true() -> bool {
    true
}

async fn add_process_rule(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<AddProcessRuleBody>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    if body.name.trim().is_empty() || body.name.len() > 64 {
        return err(StatusCode::BAD_REQUEST, "进程名 1-64 字符");
    }
    let _ = state.store.with(|c| {
        c.execute(
            "INSERT INTO process_rules (server_id, name, exact, display_name) VALUES (?1,?2,?3,?4)",
            rusqlite::params![id, body.name.trim(), body.exact as i64, body.display_name],
        )?;
        Ok(())
    });
    state.push_process_rules(&id);
    Json(json!({ "ok": true })).into_response()
}

async fn delete_process_rule(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(rule_id): Path<i64>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let sid: Option<String> = state
        .store
        .with(|c| {
            Ok(c.query_row("SELECT server_id FROM process_rules WHERE id = ?1", [rule_id], |r| r.get(0)).ok())
        })
        .unwrap_or(None);
    let _ = state.store.with(|c| {
        c.execute("DELETE FROM process_rules WHERE id = ?1", [rule_id])?;
        Ok(())
    });
    if let Some(sid) = sid {
        state.push_process_rules(&sid);
    }
    Json(json!({ "ok": true })).into_response()
}


// ---------- 站点设置 ----------

async fn get_settings(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    Json(serde_json::to_value(state.settings()).unwrap()).into_response()
}

#[derive(Deserialize)]
struct SettingsBody {
    site_name: Option<String>,
    site_description: Option<String>,
    public_page_enabled: Option<bool>,
}

async fn put_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(body): Json<SettingsBody>,
) -> impl IntoResponse {
    let Ok(admin) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    let mut s = state.settings();
    if let Some(v) = body.site_name {
        let v = v.trim().to_string();
        if v.is_empty() || v.chars().count() > 32 {
            return err(StatusCode::BAD_REQUEST, "站点名称 1-32 字符");
        }
        s.site_name = v;
    }
    if let Some(v) = body.site_description {
        if v.chars().count() > 160 {
            return err(StatusCode::BAD_REQUEST, "描述 ≤160 字符");
        }
        s.site_description = v;
    }
    if let Some(v) = body.public_page_enabled {
        s.public_page_enabled = v;
    }
    state.set_settings(&s);
    auth::audit_state(&state, Some(&admin), "settings", "更新站点设置", &auth::client_ip(&headers, Some(peer)));
    Json(json!({ "ok": true })).into_response()
}
