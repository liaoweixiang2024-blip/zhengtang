//! 内嵌前端静态资源（rust-embed）+ SPA fallback。
//!
//! 开发模式（debug 构建）下 rust-embed 直接读磁盘 `frontend/dist`，
//! 配合 `npm run dev` 时前端走 Vite 代理；生产模式编译期嵌入。

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
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
        }
        // SPA fallback：非 /api、/ws 路径一律回 index.html（前端路由）
        None => match Assets::get("index.html") {
            Some(index) => Html(index.data).into_response(),
            None => (StatusCode::NOT_FOUND, "frontend not built").into_response(),
        },
    }
}
