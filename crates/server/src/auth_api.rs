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
        // 修改自己的密码（admin/viewer 均可）
        .route("/api/auth/password", post(change_password))
        // 管理（admin）
        .route("/api/admin/servers", get(admin_servers).post(add_server))
        .route("/api/admin/servers/{id}", patch(update_server).delete(delete_server))
        // 手动排序（后台拖拽调序，仪表盘卡片顺序同步）
        .route("/api/admin/servers/reorder", post(reorder_servers))
        .route("/api/admin/groups", get(list_groups).post(add_group))
        .route("/api/admin/groups/{id}", delete(delete_group))
        .route("/api/admin/users", get(list_users).post(add_user))
        .route("/api/admin/users/{id}", delete(delete_user))
        // admin 重置他人密码（目标全部会话下线）
        .route("/api/admin/users/{id}/reset-password", post(reset_password))
        .route("/api/admin/audit", get(list_audit))
        .route("/api/admin/fingerprint", get(get_fingerprint))
        .route("/api/admin/settings", get(get_settings).put(put_settings))
        // 站点 LOGO（multipart 上传；body 上限 1.5MB）
        .route(
            "/api/admin/logo",
            post(upload_logo).delete(remove_logo).layer(axum::extract::DefaultBodyLimit::max(1024 * 1536)),
        )
        // 在线更新（探测最新版 / 下载校验替换并热切换）
        .route("/api/admin/update/check", get(update_check))
        .route("/api/admin/update/run", post(update_run))
        // 告警
        .route("/api/alerts/events", get(alert_events))
        .route("/api/admin/channels", get(list_channels).post(add_channel))
        .route("/api/admin/channels/{id}", delete(delete_channel).patch(update_channel))
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
        // 地区自动识别（按 agent 连接 IP 查询归属地）
        .route("/api/admin/servers/{id}/detect-region", post(detect_region))
}

/// 判断是否私网/回环地址（无法公网定位）。
pub(crate) fn is_private_ip(ip: &str) -> bool {
    let ip = ip.trim();
    if ip == "::1" || ip.starts_with("fe80:") || ip.starts_with("fc") || ip.starts_with("fd") {
        return true;
    }
    let v4 = ip.trim_start_matches("::ffff:");
    let parts: Vec<u32> = v4.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() == 4 {
        return parts[0] == 127
            || parts[0] == 10
            || parts[0] == 0
            || (parts[0] == 192 && parts[1] == 168)
            || (parts[0] == 172 && (16..=31).contains(&parts[1]))
            || (parts[0] == 169 && parts[1] == 254);
    }
    false
}

/// ISO 国家码 → 国旗 emoji（🇭🇰）
fn flag_of(code: &str) -> String {
    let c: Vec<char> = code.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if c.len() < 2 {
        return String::new();
    }
    let a = 0x1F1E6 + (c[0] as u32 - 'A' as u32);
    let b = 0x1F1E6 + (c[1] as u32 - 'A' as u32);
    format!("{}{}", char::from_u32(a).unwrap_or('🇦'), char::from_u32(b).unwrap_or('🇦'))
}

/// 地区自动识别：按该机 agent 最近连接 IP 查询归属地（ip-api.com，仅传 IP），
/// 成功则写入 region（"🇭🇰 香港" 格式）并广播。
async fn detect_region(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let ip: Option<String> = state
        .store
        .with(|c| Ok(c.query_row("SELECT last_ip FROM servers WHERE id = ?1", [&id], |r| r.get(0)).ok()))
        .ok()
        .flatten();
    let Some(ip) = ip.filter(|ip| !ip.is_empty()) else {
        return err(StatusCode::BAD_REQUEST, "该服务器还没有连接记录，无法识别");
    };
    if is_private_ip(&ip) {
        return err(
            StatusCode::BAD_REQUEST,
            &format!(
                "连接 IP {ip} 是内网地址，无法识别地区。\n面板与 agent 之间经过了内网穿透（如 frp）或反代，真实公网 IP 没有传到面板；\
                 反代场景可在反代层加 X-Forwarded-For 头，纯 TCP 穿透场景请直接在地区下拉中手动选择。"
            ),
        );
    }
    // ip-api.com 免费接口（无 key，仅传 IP 与语言参数；5s 超时）
    let url = format!("http://ip-api.com/json/{ip}?fields=status,countryCode,country&lang=zh-CN");
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => return err(StatusCode::BAD_GATEWAY, &format!("查询失败：{e}")),
    };
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, &format!("查询失败：{e}")),
    };
    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => return err(StatusCode::BAD_GATEWAY, &format!("解析失败：{e}")),
    };
    if json["status"].as_str() != Some("success") {
        return err(StatusCode::BAD_GATEWAY, "归属地服务查询失败");
    }
    let code = json["countryCode"].as_str().unwrap_or("").to_uppercase();
    let country = json["country"].as_str().unwrap_or(&code).to_string();
    if code.len() != 2 {
        return err(StatusCode::BAD_GATEWAY, "归属地返回异常");
    }
    let region = if country.is_empty() { code.clone() } else { format!("{} {}", flag_of(&code), country) };
    let _ = state.store.with(|c| {
        c.execute("UPDATE servers SET region = ?1 WHERE id = ?2", rusqlite::params![region, id])?;
        Ok(())
    });
    if let Some(v) = state.server_view(&id) {
        let _ = state.dash_tx.send(crate::state::DashboardEvent::Update { server: v });
    }
    tracing::info!(server = %id, ip = %ip, region = %region, "地区自动识别完成");
    (StatusCode::OK, Json(json!({ "ok": true, "region": region }))).into_response()
}

// ---------- 站点 LOGO ----------

/// LOGO 上传：multipart 单文件；位图白名单（png/jpg/webp/gif/ico，≤1MB）。
/// 存 data/uploads/logo-{hex}.{ext}，写入 settings.site_logo 并清掉旧文件。
/// 不收 SVG：同源直开 SVG 可执行脚本，位图无此面。
async fn upload_logo(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    mut mp: axum::extract::Multipart,
) -> axum::response::Response {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let field = match mp.next_field().await {
        Ok(Some(f)) => f,
        _ => return err(StatusCode::BAD_REQUEST, "缺少上传文件（multipart 字段 file）"),
    };
    let filename = field.file_name().unwrap_or("").to_lowercase();
    let ext = filename.rsplit('.').next().unwrap_or("").to_string();
    const ALLOWED: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "ico"];
    if !ALLOWED.contains(&ext.as_str()) {
        return err(StatusCode::BAD_REQUEST, "仅支持 png / jpg / webp / gif / ico 位图");
    }
    let data = match field.bytes().await {
        Ok(d) => d,
        Err(e) => return err(StatusCode::BAD_REQUEST, &format!("读取失败：{e}")),
    };
    if data.is_empty() || data.len() > 1024 * 1024 {
        return err(StatusCode::BAD_REQUEST, "文件大小需在 1KB - 1MB 之间");
    }
    let mut rnd = [0u8; 4];
    rand::rng().fill_bytes(&mut rnd);
    let name = format!("logo-{}.{ext}", data_encoding::HEXLOWER.encode(&rnd));
    let dir = state.data_dir.join("uploads");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("创建目录失败：{e}"));
    }
    if let Err(e) = std::fs::write(dir.join(&name), &data) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("写入失败：{e}"));
    }
    set_logo_setting(&state, &format!("/uploads/{name}"));
    (StatusCode::OK, Json(json!({ "ok": true, "logo": format!("/uploads/{name}") }))).into_response()
}

/// 恢复默认 LOGO：清设置并删文件。
async fn remove_logo(State(state): State<Arc<AppState>>, jar: CookieJar) -> axum::response::Response {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    set_logo_setting(&state, "");
    (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
}

/// 更新 settings.site_logo；顺带删除被替换下来的旧文件（换一次删一次，目录不积灰）。
fn set_logo_setting(state: &AppState, url: &str) {
    let old = state.settings().site_logo;
    let _ = state.store.with(|c| {
        if url.is_empty() {
            c.execute("DELETE FROM settings WHERE key = 'site_logo'", [])?;
        } else {
            c.execute(
                "INSERT INTO settings (key, value) VALUES ('site_logo', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [url],
            )?;
        }
        Ok(())
    });
    if let Some(f) = old.strip_prefix("/uploads/") {
        let _ = std::fs::remove_file(state.data_dir.join("uploads").join(f));
    }
}

// ---------- 在线更新 ----------

/// 探测最新版本：302 Location 技巧（不走 api.github.com，镜像前缀也能用）。
async fn update_check(State(state): State<Arc<AppState>>, jar: CookieJar) -> axum::response::Response {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    let target = crate::update::build_target();
    let base = crate::update::update_base(&state);
    let latest = match target {
        Some(t) => crate::update::latest_version(&base, t).await,
        None => None,
    };
    (
        StatusCode::OK,
        Json(json!({
            "current": env!("CARGO_PKG_VERSION"),
            "latest": latest,          // None = 探测不到（镜像直回 200 / 超时），前端转手动输入
            "target": target,
            "base": base,
            "updating": crate::update::UPDATING.load(std::sync::atomic::Ordering::Relaxed),
        })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct UpdateRunBody {
    version: String,
}

/// 执行更新：下载 → sha256 校验 → 解包暂存 → 响应写回后延时 exec 热切换。
/// 版本号必须显式（镜像 latest 缓存不可信）。
async fn update_run(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Json(body): Json<UpdateRunBody>,
) -> axum::response::Response {
    if let Err(e) = require_admin(&state, &jar) {
        return e;
    }
    if crate::update::UPDATING
        .swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        return err(StatusCode::CONFLICT, "已有一次更新在进行中");
    }
    let staged = match crate::update::download(&state, &body.version).await {
        Ok(p) => p,
        Err(e) => {
            crate::update::UPDATING.store(false, std::sync::atomic::Ordering::Relaxed);
            return err(StatusCode::BAD_GATEWAY, &e);
        }
    };
    tracing::info!(version = %body.version, "在线更新包就绪，1.5s 后热切换");
    // 先让本函数把 200 响应写回连接，再在后台 exec 替换进程镜像
    tokio::task::spawn_blocking(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if let Err(e) = crate::update::replace_and_exec(&staged) {
            tracing::error!(%e, "热切换失败（旧二进制继续运行，未损坏）");
            crate::update::UPDATING.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    });
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "version": body.version, "note": "热切换中，几秒内服务恢复" })),
    )
        .into_response()
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

// ---------- 密码 ----------

#[derive(Deserialize)]
struct ChangePasswordBody {
    old_password: String,
    new_password: String,
}

/// 修改自己的密码：验旧 → 换新 → 清掉本用户的其他会话（当前会话保留，
/// 免得改完密码立刻被踢回登录页）。任何登录用户可用。
async fn change_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(body): Json<ChangePasswordBody>,
) -> axum::response::Response {
    let Some(user) = auth::current_user(&state, &jar) else {
        return err(StatusCode::UNAUTHORIZED, "未登录");
    };
    if body.new_password.len() < 8 {
        return err(StatusCode::BAD_REQUEST, "新密码至少 8 位");
    }
    let cur_token = jar
        .get(auth::SESSION_COOKIE)
        .map(|c| c.value().to_string())
        .unwrap_or_default();
    let hash: Option<String> = state
        .store
        .with(|c| {
            Ok(c.query_row("SELECT password_hash FROM users WHERE id = ?1", [user.id], |r| r.get(0)).ok())
        })
        .ok()
        .flatten();
    let Some(hash) = hash else {
        return err(StatusCode::NOT_FOUND, "用户不存在");
    };
    if !auth::verify_password(&hash, &body.old_password) {
        auth::audit_state(&state, Some(&user), "password_fail", "原密码错误", &auth::client_ip(&headers, Some(peer)));
        return err(StatusCode::FORBIDDEN, "原密码不正确");
    }
    let new_hash = match auth::hash_password(&body.new_password) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("密码哈希失败：{e}")),
    };
    let _ = state.store.with(|c| {
        c.execute("UPDATE users SET password_hash = ?1 WHERE id = ?2", rusqlite::params![new_hash, user.id])?;
        c.execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND token != ?2",
            rusqlite::params![user.id, cur_token],
        )?;
        Ok(())
    });
    auth::audit_state(&state, Some(&user), "password_change", "修改了自己的密码", &auth::client_ip(&headers, Some(peer)));
    (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
}

/// admin 重置他人密码：目标用户全部会话下线（改完必须用新密码重新登录）。
async fn reset_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Json(body): Json<ChangePasswordBody>,
) -> axum::response::Response {
    let Ok(admin) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    if body.new_password.len() < 8 {
        return err(StatusCode::BAD_REQUEST, "新密码至少 8 位");
    }
    let new_hash = match auth::hash_password(&body.new_password) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("密码哈希失败：{e}")),
    };
    let updated = state
        .store
        .with(|c| {
            let n = c.execute("UPDATE users SET password_hash = ?1 WHERE id = ?2", rusqlite::params![new_hash, id])?;
            c.execute("DELETE FROM sessions WHERE user_id = ?1", [id])?;
            Ok(n)
        })
        .unwrap_or(0);
    if updated == 0 {
        return err(StatusCode::NOT_FOUND, "用户不存在");
    }
    auth::audit_state(&state, Some(&admin), "password_reset", &format!("重置用户 {id} 的密码"), &auth::client_ip(&headers, Some(peer)));
    (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
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
struct ReorderBody {
    /// 新顺序的完整 id 列表
    ids: Vec<String>,
}

/// 手动排序：按传入顺序写 sort_order（1..n），并广播给仪表盘重建列表顺序。
async fn reorder_servers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(body): Json<ReorderBody>,
) -> axum::response::Response {
    let Ok(admin) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    if body.ids.is_empty() || body.ids.len() > 1000 {
        return err(StatusCode::BAD_REQUEST, "ids 为空或过多");
    }
    let _ = state.store.with(|c| {
        for (i, id) in body.ids.iter().enumerate() {
            c.execute(
                "UPDATE servers SET sort_order = ?1 WHERE id = ?2",
                rusqlite::params![(i + 1) as i64, id],
            )?;
        }
        Ok(())
    });
    auth::audit_state(&state, Some(&admin), "reorder", &format!("调整主机顺序（{} 台）", body.ids.len()), &auth::client_ip(&headers, Some(peer)));
    let _ = state.dash_tx.send(crate::state::DashboardEvent::Resort { ids: body.ids });
    (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
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

    // 安装令牌（24h 限时；/a/{token} 据此下发免参数安装脚本；删除服务器即吊销）
    let mut token_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut token_bytes);
    let install_token = data_encoding::HEXLOWER.encode(&token_bytes);
    let token_expires = unix_now() + 24 * 3600;
    let _ = state.store.with(|c| {
        crate::store::insert_server(c, &agent_id, &body.name, &pubkey)?;
        c.execute(
            "UPDATE servers SET secret_key = ?1, install_token = ?2, install_token_expires = ?3 WHERE id = ?4",
            rusqlite::params![secret_hex, install_token, token_expires, agent_id],
        )?;
        Ok(())
    });
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
        // 短安装命令：面板注入参数，目标机免参数执行。
        // base 可能是 wss:// 口径（前端为 agent 连接做的转换）；curl 下载必须用 http(s)。
        // 用 bash 执行：Ubuntu 的 sh 是 dash，pipefail 等语法不兼容（脚本本身也已 POSIX 化双保险）
        "install_cmd_short": format!(
            "curl -kfsSL {}/a/{} | sudo bash",
            base.replace("wss://", "https://").replace("ws://", "http://"),
            install_token
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
            // 合法值：1-31（29/30/31 在不足的月份按当月最后一天）、99=每月月末、NULL=自然月
            if let Some(d) = rd {
                if !(1..=31).contains(&d) && d != 99 {
                    anyhow::bail!("流量重置日必须是 1-31、99（月末）或留空");
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
            let mut stmt = c.prepare("SELECT id, type, name, config, enabled FROM notify_channels ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| {
                    let cfg: String = r.get(3)?;
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "type": r.get::<_, String>(1)?,
                        "name": r.get::<_, String>(2)?,
                        "config": serde_json::from_str::<serde_json::Value>(&cfg).unwrap_or(serde_json::Value::Null),
                        "enabled": r.get::<_, i64>(4)? != 0,
                    }))
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

#[derive(Deserialize)]
struct UpdateChannelBody {
    name: Option<String>,
    config: Option<serde_json::Value>,
    enabled: Option<bool>,
}

/// 编辑渠道：改名 / 改配置 / 启停（不动的字段保持原值）。
async fn update_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Json(body): Json<UpdateChannelBody>,
) -> impl IntoResponse {
    let Ok(admin) = require_admin(&state, &jar) else {
        return err(StatusCode::FORBIDDEN, "需要管理员权限");
    };
    let rows = state
        .store
        .with(|c| {
            let mut n = 0;
            if let Some(v) = &body.name {
                n += c.execute("UPDATE notify_channels SET name = ?1 WHERE id = ?2", rusqlite::params![v.trim(), id])?;
            }
            if let Some(v) = &body.config {
                n += c.execute(
                    "UPDATE notify_channels SET config = ?1 WHERE id = ?2",
                    rusqlite::params![v.to_string(), id],
                )?;
            }
            if let Some(v) = body.enabled {
                n += c.execute(
                    "UPDATE notify_channels SET enabled = ?1 WHERE id = ?2",
                    rusqlite::params![if v { 1 } else { 0 }, id],
                )?;
            }
            Ok(n)
        })
        .unwrap_or(0);
    if rows == 0 {
        return err(StatusCode::NOT_FOUND, "渠道不存在或没有可更新字段");
    }
    auth::audit_state(&state, Some(&admin), "update_channel", &format!("渠道 {id}"), &auth::client_ip(&headers, Some(peer)));
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
    globe_enabled: Option<bool>,
    /// 在线更新源（空 = GitHub 官方；长度上限防滥用）
    update_base: Option<String>,
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
    if let Some(v) = body.globe_enabled {
        s.globe_enabled = v;
    }
    if let Some(v) = body.public_page_enabled {
        s.public_page_enabled = v;
    }
    if let Some(v) = body.update_base {
        let v = v.trim().to_string();
        if v.len() > 256 {
            return err(StatusCode::BAD_REQUEST, "更新源地址过长");
        }
        // 只接受 https:// 前缀（空串恢复默认）
        if !v.is_empty() && !v.starts_with("https://") && !v.starts_with("http://") {
            return err(StatusCode::BAD_REQUEST, "更新源必须以 http(s):// 开头");
        }
        s.update_base = v;
    }
    state.set_settings(&s);
    auth::audit_state(&state, Some(&admin), "settings", "更新站点设置", &auth::client_ip(&headers, Some(peer)));
    Json(json!({ "ok": true })).into_response()
}
