//! Routes, static files, the blob endpoint, the proxy route and security
//! headers of the app origin.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Router, middleware};
use nail_api::ErrorCode;
use nail_core::Service;
use nail_proxy::Proxy;
use tower_http::services::{ServeDir, ServeFile};

use crate::TestControl;
use crate::acme::AcmeChallenges;
use crate::guard::{self, Guard, HostAllowlist};
use crate::headers::{self, HeaderProfile, HeaderSet};
use crate::ws;

/// Blobs are raw mail: never rendered by the browser as a page.
const BLOB_CSP: &str = "default-src 'none'; sandbox";

/// Everything that shapes the app router.
#[derive(Clone)]
pub(crate) struct AppConfig {
    pub(crate) ui_dir: Option<PathBuf>,
    pub(crate) control: Option<Arc<dyn TestControl>>,
    pub(crate) hosts: HostAllowlist,
    pub(crate) profile: HeaderProfile,
    pub(crate) tls: bool,
    pub(crate) sandbox_origins: Vec<String>,
    pub(crate) proxy: Option<Arc<Proxy>>,
    pub(crate) acme: Option<AcmeChallenges>,
    pub(crate) auth: Option<nail_remote_auth::RemoteAuth>,
}

impl AppConfig {
    /// Loopback plain HTTP on `port`, compat headers, no proxy.
    pub(crate) fn loopback(
        ui_dir: Option<PathBuf>,
        port: u16,
        control: Option<Arc<dyn TestControl>>,
    ) -> Self {
        Self {
            ui_dir,
            control,
            hosts: HostAllowlist::new(port),
            profile: HeaderProfile::Compat,
            tls: false,
            sandbox_origins: Vec::new(),
            proxy: None,
            acme: None,
            auth: None,
        }
    }
}

/// The axum router for `service`, serving the UI from `ui_dir` (if any).
///
/// `port` is the port the server is bound to: every request must name a
/// loopback host with that port in `Host` (and in `Origin`, if present);
/// anything else is refused with `403` (DNS-rebinding protection, see
/// [`crate::guard`]).
pub fn router(service: Service, ui_dir: Option<PathBuf>, port: u16) -> Router {
    router_with_control(service, ui_dir, port, None)
}

/// [`router`] plus, when `control` is given, the test hooks at
/// `POST /api/test/{command}` (same host checks as every route).
pub fn router_with_control(
    service: Service,
    ui_dir: Option<PathBuf>,
    port: u16,
    control: Option<Arc<dyn TestControl>>,
) -> Router {
    app_router(service, AppConfig::loopback(ui_dir, port, control))
}

/// `GET /api/proxy/{token}` answered by `proxy` (both origins).
pub(crate) fn proxy_routes<S: Clone + Send + Sync + 'static>(proxy: Arc<Proxy>) -> Router<S> {
    Router::new()
        .route("/api/proxy/{token}", get(proxy_resource))
        .with_state((proxy, Arc::new(crate::proxy_limit::RateLimiter::default())))
}

/// The rate-limit key of a proxy request: its remote session, else the
/// client address (the sandbox origin's documents send no cookies).
fn limit_key(extensions: &axum::http::Extensions) -> String {
    if let Some(ctx) = extensions.get::<nail_remote_auth::RemoteSessionCtx>() {
        return format!("session:{}", ctx.session.id);
    }
    extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map_or_else(
            || "client:unknown".to_owned(),
            |info| format!("client:{}", info.0.ip()),
        )
}

pub(crate) fn app_router(service: Service, config: AppConfig) -> Router {
    // What needs a session in the remote UI: the protocol, raw mail and
    // the proxy.
    let mut session_routes = Router::new()
        .route("/api/ws", get(ws::upgrade))
        .route("/api/blob/{hash}", get(blob));
    if let Some(proxy) = config.proxy {
        session_routes = session_routes.merge(proxy_routes(proxy));
    }
    let mut session_routes = session_routes.with_state(service.clone());
    let mut api = Router::new();
    if let Some(auth) = config.auth {
        session_routes = session_routes.layer(middleware::from_fn_with_state(
            auth.clone(),
            nail_remote_auth::http::require_session,
        ));
        api = api.merge(nail_remote_auth::http::router(auth));
    }
    if let Some(control) = config.control {
        let test = Router::new()
            .route("/api/test/{command}", post(test_control))
            .with_state(control);
        api = api.merge(test);
    }
    if let Some(challenges) = config.acme {
        let acme = Router::new()
            .route("/.well-known/acme-challenge/{token}", get(acme_challenge))
            .with_state(challenges);
        api = api.merge(acme);
    }
    let api = api
        .merge(session_routes)
        .route("/api/{*rest}", any(not_found));

    let app = match config.ui_dir {
        Some(dir) => {
            let index = dir.join("index.html");
            api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        }
        None => api.fallback(not_found),
    };

    let header_set = Arc::new(HeaderSet::app(
        config.profile,
        config.tls,
        &config.sandbox_origins,
    ));
    app.layer(middleware::from_fn_with_state(
        Guard {
            hosts: config.hosts,
            fetch_metadata: true,
        },
        guard::check,
    ))
    .layer(middleware::from_fn_with_state(header_set, headers::apply))
    .layer(middleware::map_request(peer_socket_addr))
}

/// Copies the peer address from the listener's [`crate::ClientAddr`] into
/// `ConnectInfo<SocketAddr>`, where `nail-remote-auth` reads it (rate
/// limits per address, audit entries).
async fn peer_socket_addr(mut request: axum::extract::Request) -> axum::extract::Request {
    use axum::extract::ConnectInfo;
    if let Some(ConnectInfo(crate::ClientAddr(addr))) = request
        .extensions()
        .get::<ConnectInfo<crate::ClientAddr>>()
        .cloned()
    {
        request.extensions_mut().insert(ConnectInfo(addr));
    }
    request
}

/// `GET /api/proxy/{token}`: a remote resource through the proxy.
async fn proxy_resource(
    State((proxy, limiter)): State<(Arc<Proxy>, Arc<crate::proxy_limit::RateLimiter>)>,
    Path(token): Path<String>,
    request: axum::extract::Request,
) -> Response {
    if !limiter.allow(&limit_key(request.extensions())) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
                (header::RETRY_AFTER, HeaderValue::from_static("60")),
            ],
        )
            .into_response();
    }
    match proxy.serve(&token).await {
        Ok(resource) => {
            let mut response = (
                [
                    (
                        header::CONTENT_TYPE,
                        HeaderValue::from_static(resource.content_type),
                    ),
                    (
                        header::CACHE_CONTROL,
                        HeaderValue::from_str(&resource.cache_control)
                            .unwrap_or(HeaderValue::from_static("no-store")),
                    ),
                    // Fonts are CORS requests from opaque-origin documents;
                    // the bytes are public and the token is the capability.
                    (
                        header::ACCESS_CONTROL_ALLOW_ORIGIN,
                        HeaderValue::from_static("*"),
                    ),
                ],
                resource.body,
            )
                .into_response();
            for (name, value) in nail_proxy::RESPONSE_HEADERS {
                response.headers_mut().insert(
                    HeaderName::from_static(name),
                    HeaderValue::from_static(value),
                );
            }
            response
        }
        Err(err) => {
            let status = StatusCode::from_u16(err.status()).unwrap_or(StatusCode::BAD_GATEWAY);
            (
                status,
                [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
            )
                .into_response()
        }
    }
}

/// `GET /.well-known/acme-challenge/{token}` (internet mode only).
async fn acme_challenge(
    State(challenges): State<AcmeChallenges>,
    Path(token): Path<String>,
) -> Response {
    match challenges.get(&token) {
        Some(key_authorization) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            key_authorization,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// `POST /api/test/{command}`: a test hook (see [`TestControl`]).
async fn test_control(
    State(control): State<Arc<dyn TestControl>>,
    Path(command): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let args: serde_json::Value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(args) => args,
            Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
        }
    };
    match control.control(&command, &args) {
        Ok(value) => (
            [(header::CONTENT_TYPE, "application/json")],
            value.to_string(),
        )
            .into_response(),
        Err(err) => (StatusCode::BAD_REQUEST, err).into_response(),
    }
}

/// `GET /api/blob/{hash}`: the raw RFC 5322 message with that SHA-256.
/// A remote session needs the `raw-source` scope and a grant for every
/// account (a blob is not tied to one message).
async fn blob(
    State(service): State<Service>,
    session: Option<axum::Extension<nail_remote_auth::RemoteSessionCtx>>,
    Path(hash): Path<String>,
) -> Response {
    if let Some(axum::Extension(ctx)) = session {
        let mail = &ctx.grant().mail;
        if !mail.has(nail_api::McpScope::RawSource) || !mail.all_accounts {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    fn service() -> (Service, tempfile::TempDir, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let mut options = nail_core::ServiceOptions::new(data.path(), config.path());
        options.watch_config = false;
        (Service::open(options).unwrap(), data, config)
    }

    async fn status(router: Router, path: &str) -> (StatusCode, Vec<u8>) {
        use http_body_util::BodyExt;
        let request = Request::get(path)
            .header("Host", "127.0.0.1:4780")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    #[tokio::test]
    async fn acme_challenges_are_served_only_while_configured() {
        let (service, _d, _c) = service();
        let without = app_router(service.clone(), AppConfig::loopback(None, 4780, None));
        assert_eq!(
            status(without, "/.well-known/acme-challenge/t1").await.0,
            StatusCode::NOT_FOUND
        );

        let challenges = AcmeChallenges::new();
        challenges.insert_for_tests("t1", "t1.thumbprint");
        let mut config = AppConfig::loopback(None, 4780, None);
        config.acme = Some(challenges);
        let with = app_router(service, config);
        assert_eq!(
            status(with.clone(), "/.well-known/acme-challenge/t1").await,
            (StatusCode::OK, b"t1.thumbprint".to_vec())
        );
        assert_eq!(
            status(with, "/.well-known/acme-challenge/other").await.0,
            StatusCode::NOT_FOUND
        );
    }
}
