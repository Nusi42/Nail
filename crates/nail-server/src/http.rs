//! Routes, static files, the blob endpoint and security headers.

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Router, middleware};
use nail_api::ErrorCode;
use nail_core::Service;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::guard::{self, HostAllowlist};
use crate::ws;

/// Content Security Policy of the app. Mirrors the desktop shell's
/// (`app/src-tauri/tauri.conf.json`): scripts only from the bundle, the
/// theme's generated `<style>` element needs inline styles, the reader
/// iframe is `srcdoc` (it additionally carries its own stricter CSP).
const APP_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; \
     frame-src 'self' blob: data:; object-src 'none'; base-uri 'none'; \
     form-action 'none'; frame-ancestors 'none'";

/// Blobs are raw mail: never rendered by the browser as a page.
const BLOB_CSP: &str = "default-src 'none'; sandbox";

/// The axum router for `service`, serving the UI from `ui_dir` (if any).
///
/// `port` is the port the server is bound to: every request must name a
/// loopback host with that port in `Host` (and in `Origin`, if present);
/// anything else is refused with `403` (DNS-rebinding protection, see
/// [`crate::guard`]).
pub fn router(service: Service, ui_dir: Option<PathBuf>, port: u16) -> Router {
    let api = Router::new()
        .route("/api/ws", get(ws::upgrade))
        .route("/api/blob/{hash}", get(blob))
        .route("/api/{*rest}", any(not_found))
        .with_state(service);

    let app = match ui_dir {
        Some(dir) => {
            let index = dir.join("index.html");
            api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        }
        None => api.fallback(not_found),
    };

    app.layer(middleware::from_fn_with_state(
        HostAllowlist::new(port),
        guard::check,
    ))
    .layer(SetResponseHeaderLayer::if_not_present(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(APP_CSP),
    ))
    .layer(SetResponseHeaderLayer::overriding(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    ))
    .layer(SetResponseHeaderLayer::overriding(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    ))
    .layer(SetResponseHeaderLayer::overriding(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    ))
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// `GET /api/blob/{hash}`: the raw RFC 5322 message with that SHA-256.
async fn blob(State(service): State<Service>, Path(hash): Path<String>) -> Response {
    let file_name = format!("attachment; filename=\"{}.eml\"", hash.escape_default());
    let result = tokio::task::spawn_blocking(move || service.blob(&hash)).await;
    let bytes = match result {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => {
            let status = match err.code() {
                ErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
                ErrorCode::NotFound => StatusCode::NOT_FOUND,
                _ => {
                    tracing::error!(%err, "reading a blob failed");
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            };
            return status.into_response();
        }
        Err(err) => {
            tracing::error!(%err, "blob task failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let disposition =
        HeaderValue::from_str(&file_name).unwrap_or(HeaderValue::from_static("attachment"));
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("message/rfc822"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=31536000, immutable"),
            ),
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(BLOB_CSP),
            ),
        ],
        Body::from(bytes),
    )
        .into_response()
}
