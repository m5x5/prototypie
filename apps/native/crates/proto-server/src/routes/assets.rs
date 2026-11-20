//! Static assets designs load relatively (`/vendor/…`, `/imports/…`), served read-only from `<data>/public/`
//! (copied from the reference app's `public/` by proto-core's import). No listings; traversal rejected.

use std::path::{Component, Path as FsPath, PathBuf};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::AppState;

pub fn public_dir(app: &AppState) -> PathBuf {
    proto_core::public_dir(&app.data_dir)
}

fn mime(path: &FsPath) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// `rest` as a safe relative path: normal components only, no hidden segments.
pub fn safe_rel(rest: &str) -> Option<PathBuf> {
    if rest.is_empty() || rest.contains('\\') || rest.contains('\0') {
        return None;
    }
    let p = FsPath::new(rest);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) if !s.to_string_lossy().starts_with('.') => out.push(s),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

async fn serve(root: PathBuf, rest: &str) -> Response {
    let Some(rel) = safe_rel(rest) else { return StatusCode::NOT_FOUND.into_response() };
    let file = root.join(&rel);
    // Symlinks must not lead out of the root.
    let (Ok(real_root), Ok(real)) = (tokio::fs::canonicalize(&root).await, tokio::fs::canonicalize(&file).await) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !real.starts_with(&real_root) || !tokio::fs::metadata(&real).await.is_ok_and(|m| m.is_file()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::read(&real).await {
        Ok(bytes) => {
            let mut res = Response::new(Body::from(bytes));
            let h = res.headers_mut();
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime(&rel)));
            h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"));
            res
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// GET /<dir>/*path for each of proto-core's `REFERENCE_ASSET_DIRS` (vendor, imports, …).
pub fn routes() -> axum::Router<AppState> {
    let mut r = axum::Router::new();
    for dir in proto_core::REFERENCE_ASSET_DIRS {
        r = r.route(
            &format!("/{dir}/{{*rest}}"),
            axum::routing::get(move |State(app): State<AppState>, Path(rest): Path<String>| async move {
                serve(public_dir(&app).join(dir), &rest).await
            }),
        );
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_hidden() {
        assert_eq!(safe_rel("a/b.js"), Some(PathBuf::from("a/b.js")));
        for bad in ["", "../x", "a/../../x", "/etc/passwd", ".env", "a/.git/config", "a\\b", "./a"] {
            assert_eq!(safe_rel(bad), None, "{bad}");
        }
        assert_eq!(mime(FsPath::new("x.WOFF2")), "font/woff2");
    }
}
