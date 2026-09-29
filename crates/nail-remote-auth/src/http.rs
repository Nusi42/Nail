//! The axum pieces `nail-server` mounts (WP45 wires them):
//!
//! * [`router`]: `POST` [`AUTH_PATH`] — the login protocol. Needs a JSON
//!   body (`Content-Type: application/json`, so a cross-site form cannot
//!   send it without a CORS preflight), an acceptable `Origin`, and — for
//!   every step but `Status` — the CSRF header.
//! * [`require_session`]: middleware for the WebSocket upgrade, the blob
//!   route and the proxy route. Resolves the session (401 without one),
//!   checks the CSRF header on state-changing methods (403), attaches
//!   [`RemoteSessionCtx`] to the request and sends a rotated session
//!   cookie with the response.
//! * [`RemoteSessionCtx`] is an extractor for handlers behind it.
//!
//! Serve with `into_make_service_with_connect_info::<SocketAddr>()` so the
//! per-address rate limits see the client's address (without it every
//! client shares the `ip:unknown` counters).

use std::net::SocketAddr;

use axum::Router;
use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HOST, ORIGIN, SET_COOKIE, USER_AGENT};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use nail_api::remote::SESSION_COOKIE;
use nail_api::remote::{AUTH_PATH, AuthFailure, AuthRequest, AuthResponse, CSRF_HEADER};
use zeroize::Zeroizing;

use crate::login::AuthReply;
use crate::service::{NoSession, RemoteAuth};
use crate::session::{ClientMeta, Cookies, RemoteSessionCtx, clear_cookie};

/// The login route, ready to merge into the server's router.
pub fn router(auth: RemoteAuth) -> Router {
    Router::new()
        .route(AUTH_PATH, post(auth_handler))
        .with_state(auth)
}

/// Who sent a request: the peer address (from `ConnectInfo`) and the
/// `User-Agent`.
pub fn client_meta(headers: &HeaderMap, peer: Option<SocketAddr>) -> ClientMeta {
    ClientMeta {
        ip: peer.map(|p| p.ip()),
        user_agent: headers
            .get(USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    }
}

fn header(headers: &HeaderMap, name: impl axum::http::header::AsHeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn peer_of(extensions: &axum::http::Extensions) -> Option<SocketAddr> {
    extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0)
}

fn json_reply(status: StatusCode, reply: AuthReply) -> Response {
    let body = serde_json::to_vec(&reply.response).unwrap_or_default();
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(token) = reply.csrf_token
        && let Ok(value) = HeaderValue::from_str(&token)
    {
        headers.insert(CSRF_HEADER, value);
    }
    for cookie in reply.cookies {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            headers.append(SET_COOKIE, value);
        }
    }
    response
}

fn bad_request(status: StatusCode) -> Response {
    json_reply(
        status,
        AuthReply {
            response: AuthResponse::Failed {
                reason: AuthFailure::BadRequest,
                retry_after_secs: None,
            },
            cookies: Vec::new(),
            csrf_token: None,
        },
    )
}

async fn auth_handler(State(auth): State<RemoteAuth>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let headers = &parts.headers;
    let is_json = header(headers, CONTENT_TYPE).is_some_and(|ct| {
        ct.split(';')
            .next()
            .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
    });
    if !is_json {
        return bad_request(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    if !auth.origin_allowed(header(headers, ORIGIN), header(headers, HOST)) {
        return bad_request(StatusCode::FORBIDDEN);
    }
    let Ok(bytes) = axum::body::to_bytes(body, auth.policy().max_body_bytes).await else {
        return bad_request(StatusCode::PAYLOAD_TOO_LARGE);
    };
    // The body holds passwords and codes: keep it in a buffer that is
    // wiped when dropped.
    let bytes = Zeroizing::new(Vec::from(bytes));
    let Ok(request) = serde_json::from_slice::<AuthRequest>(&bytes) else {
        return bad_request(StatusCode::OK);
    };
    drop(bytes);
    let cookies = Cookies::from_headers(headers);
    let client = client_meta(headers, peer_of(&parts.extensions));
    let reply = auth
        .handle_auth(request, &cookies, &client, header(headers, CSRF_HEADER))
        .await;
    json_reply(StatusCode::OK, reply)
}

fn unauthorized(clear: bool) -> Response {
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    if clear && let Ok(value) = HeaderValue::from_str(&clear_cookie(SESSION_COOKIE)) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Middleware: only requests with a valid session pass (see the module
/// docs). Use with `axum::middleware::from_fn_with_state(auth, require_session)`.
pub async fn require_session(
    State(auth): State<RemoteAuth>,
    mut request: Request,
    next: Next,
) -> Response {
    let cookies = Cookies::from_headers(request.headers());
    let client = client_meta(request.headers(), peer_of(request.extensions()));
    let resolved = match auth.resolve(&cookies, &client) {
        Ok(resolved) => resolved,
        Err(NoSession::Missing) => return unauthorized(false),
        Err(NoSession::Invalid) => return unauthorized(true),
    };
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !safe && !auth.check_csrf(&cookies, header(request.headers(), CSRF_HEADER)) {
        return StatusCode::FORBIDDEN.into_response();
    }
    request.extensions_mut().insert(resolved.ctx);
    let mut response = next.run(request).await;
    if let Some(cookie) = resolved.set_cookie
        && let Ok(value) = HeaderValue::from_str(&cookie)
    {
        response.headers_mut().append(SET_COOKIE, value);
    }
    response
}

impl<S: Send + Sync> FromRequestParts<S> for RemoteSessionCtx {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<RemoteSessionCtx>()
            .cloned()
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}
