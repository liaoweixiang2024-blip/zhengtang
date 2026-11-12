//! 内嵌前端静态资源（rust-embed）+ SPA fallback。
//!
//! 缓存策略：
//! - index.html：no-cache（每次校验，保证拿到最新 hash 资源引用）
//! - 带 hash 的 /assets/*：immutable 长缓存（内容变即换名）
//! - 其余：no-cache 兜底

use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../frontend/dist"]
struct Assets;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(file) => serve_file(path, file.data),
        // 带 hash 的静态资源缺失 = 客户端缓存错位，返回真 404 让浏览器重新拉 index
        // （fallback 成 HTML 会被当 JS 解析导致白屏）
        None if path.starts_with("assets/") => {
            (StatusCode::NOT_FOUND, "asset not found (硬刷新页面)").into_response()
        }
        // SPA fallback：其余非 /api、/ws 路径回 index.html（前端路由）
        None => match Assets::get("index.html") {
            Some(index) => Html(index.data)
                .into_response()
                // fallback 页也可能是任意路由，绝不能缓存
                .cache_control_no_store(),
            None => (StatusCode::NOT_FOUND, "frontend not built").into_response(),
        },
    }
}

fn serve_file(path: &str, data: std::borrow::Cow<'static, [u8]>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut resp = ([(header::CONTENT_TYPE, mime.as_ref())], data).into_response();
    let headers = resp.headers_mut();
    // Vite 产物 assets/* 文件名带内容 hash → 可永久缓存；其余（index.html 等）禁缓存
    if path.starts_with("assets/") {
        headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("public, max-age=31536000, immutable"));
    } else {
        headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-cache"));
    }
    resp
}

/// 给 Response 设置 no-store 的轻量 trait。
trait NoStore {
    fn cache_control_no_store(self) -> Self;
}

impl NoStore for Response {
    fn cache_control_no_store(mut self) -> Self {
        self.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        self
    }
}
