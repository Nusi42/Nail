//! The axum pieces: the login route (content type, origin, CSRF, cookies)
//! and the session middleware / extractor, driven through `tower`.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_TYPE, COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::{Request, Response, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use common::*;
use nail_api::remote::{AUTH_PATH, AuthFailure, AuthResponse, CSRF_HEADER};
use nail_remote_auth::RemoteSessionCtx;
use nail_remote_auth::http::{require_session, router};
use tower::ServiceExt as _;

/// The login route plus two protected routes, like `nail-server` mounts
/// them.
fn app(h: &Harness) -> Router {
    let protected = Router::new()
        .route(
            "/api/whoami",
            get(|ctx: RemoteSessionCtx| async move { ctx.session.user_name }),
        )
        .route("/api/write", post(|| async { "written" }))
        .layer(from_fn_with_state(h.auth.clone(), require_session));
    router(h.auth.clone()).merge(protected)
}

/// A browser at the HTTP level: a cookie jar and the CSRF token.
struct Client {
    cookies: Vec<(String, String)>,
    csrf: Option<String>,
    peer: SocketAddr,
}

impl Client {
    fn new(peer: &str) -> Self {
        Self {
            cookies: Vec::new(),
            csrf: None,
            peer: peer.parse().unwrap(),
        }
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn keep(&mut self, response: &Response<Body>) {
        for header in response.headers().get_all(SET_COOKIE) {
            let cookie = cookie::Cookie::parse(header.to_str().unwrap().to_owned()).unwrap();
            self.cookies.retain(|(n, _)| n != cookie.name());
            if !cookie
                .max_age()
                .is_some_and(cookie::time::SignedDuration::is_zero)
            {
                self.cookies
                    .push((cookie.name().to_owned(), cookie.value().to_owned()));
            }
        }
    }

    fn request(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, "nail.local:4780")
            .header(ORIGIN, common::ORIGIN)
            .header(COOKIE, self.cookie_header());
        if let Some(csrf) = &self.csrf {
            builder = builder.header(CSRF_HEADER, csrf);
        }
        let body = match body {
            Some(body) => {
                builder = builder.header(CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let mut request = builder.body(body).unwrap();
        request.extensions_mut().insert(ConnectInfo(self.peer));
        request
    }

    /// Pairs this browser as "Phone" with `code`.
    async fn pair(&mut self, app: &Router, code: &str) -> Response<Body> {
        let request = self.request(
            "POST",
            AUTH_PATH,
            Some(serde_json::json!({"type": "Pair", "code": code, "device_name": "Phone"})),
        );
        self.send(app, request).await
    }

    async fn send(&mut self, app: &Router, request: Request<Body>) -> Response<Body> {
        let response = app.clone().oneshot(request).await.unwrap();
        self.keep(&response);
        response
    }

    async fn auth(&mut self, app: &Router, body: serde_json::Value) -> (StatusCode, AuthResponse) {
        let request = self.request("POST", AUTH_PATH, Some(body));
        let response = self.send(app, request).await;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn fetch_csrf(&mut self, app: &Router) {
        let (_, response) = self.auth(app, serde_json::json!({"type": "Status"})).await;
        let AuthResponse::Status { status } = response else {
            panic!("{response:?}");
        };
        self.csrf = Some(status.csrf_token);
    }
}

async fn text(response: Response<Body>) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// A harness with its app, a pairing code for `ada` and a client that has
/// its CSRF token.
async fn pairing_client() -> (Harness, Router, String, Client) {
    let h = Harness::new();
    let app = app(&h);
    let ada = h.user("ada", false).await;
    let code = h.code(&ada, None);
    let mut client = Client::new("192.0.2.10:50000");
    client.fetch_csrf(&app).await;
    (h, app, code, client)
}

#[tokio::test]
async fn pairing_over_http_sets_hardened_cookies() {
    let (h, app, code, mut client) = pairing_client().await;

    let response = client.pair(&app, &code).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let set_cookies: Vec<String> = response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    assert_eq!(set_cookies.len(), 2);
    for cookie in &set_cookies {
        for part in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
            assert!(cookie.contains(part), "{cookie}");
        }
        assert!(cookie.starts_with("__Host-nail-"), "{cookie}");
    }
    let body = text(response).await;
    assert!(body.contains("\"type\":\"LoggedIn\""), "{body}");
    // The session records the peer address from ConnectInfo.
    let sessions = h
        .store
        .list_remote_sessions(&nail_api::remote::RemoteSessionFilter::default(), h.now())
        .unwrap();
    assert_eq!(sessions[0].ip.as_deref(), Some("192.0.2.10"));

    // The protected route now answers.
    let response = client
        .send(&app, client.request("GET", "/api/whoami", None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(text(response).await, "ada");
}

#[tokio::test]
async fn the_login_route_refuses_forms_and_foreign_origins() {
    let h = Harness::new();
    let app = app(&h);
    let client = Client::new("192.0.2.10:50000");

    // A cross-site form post (text/plain, no preflight).
    let mut request = client.request("POST", AUTH_PATH, None);
    request
        .headers_mut()
        .insert(CONTENT_TYPE, "text/plain".parse().unwrap());
    *request.body_mut() = Body::from(r#"{"type":"Status"}"#);
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Another origin.
    let mut request = client.request(
        "POST",
        AUTH_PATH,
        Some(serde_json::json!({"type": "Status"})),
    );
    request
        .headers_mut()
        .insert(ORIGIN, "https://evil.example".parse().unwrap());
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // No origin at all.
    let mut request = client.request(
        "POST",
        AUTH_PATH,
        Some(serde_json::json!({"type": "Status"})),
    );
    request.headers_mut().remove(ORIGIN);
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Garbage JSON: a protocol-level failure.
    let mut request = client.request("POST", AUTH_PATH, Some(serde_json::json!({"type": "Nope"})));
    *request.body_mut() = Body::from("{not json");
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = text(response).await;
    assert!(body.contains("bad-request"), "{body}");

    // A huge body.
    let mut request = client.request(
        "POST",
        AUTH_PATH,
        Some(serde_json::json!({"type": "Status"})),
    );
    *request.body_mut() = Body::from(vec![b' '; 100 * 1024]);
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn origin_defaults_to_the_host_when_none_are_configured() {
    let h = Harness::new();
    h.auth.set_config(nail_remote_auth::AuthConfig {
        origins: Vec::new(),
        ..config()
    });
    assert!(
        h.auth
            .origin_allowed(Some("https://nail.local:4780"), Some("nail.local:4780"))
    );
    assert!(
        h.auth
            .origin_allowed(Some("http://127.0.0.1:4780"), Some("127.0.0.1:4780"))
    );
    assert!(
        !h.auth
            .origin_allowed(Some("https://evil.example"), Some("nail.local:4780"))
    );
    assert!(!h.auth.origin_allowed(Some("null"), Some("nail.local:4780")));
    assert!(!h.auth.origin_allowed(None, Some("nail.local:4780")));
    assert!(!h.auth.origin_allowed(Some("https://nail.local:4780"), None));
}

#[tokio::test]
async fn protected_routes_need_a_session_and_csrf_for_writes() {
    let h = Harness::new();
    let app = app(&h);
    let ada = h.user("ada", false).await;
    let mut client = Client::new("192.0.2.10:50000");

    let response = client
        .send(&app, client.request("GET", "/api/whoami", None))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Pair (through the harness-level browser) and copy its cookies.
    let mut browser = h.paired(&ada, "192.0.2.10").await;
    client.cookies = vec![
        (
            "__Host-nail-session".into(),
            browser.session.clone().unwrap(),
        ),
        ("__Host-nail-device".into(), browser.device.clone().unwrap()),
    ];
    let response = client
        .send(&app, client.request("GET", "/api/whoami", None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    // A POST without the CSRF header is refused, with it allowed.
    let response = client
        .send(&app, client.request("POST", "/api/write", None))
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    client.csrf = browser.csrf.clone();
    let response = client
        .send(&app, client.request("POST", "/api/write", None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    // A tampered session cookie: 401 and the cookie is cleared.
    let saved = client.cookies.clone();
    client.cookies[0].1 = nail_remote_auth::secret::random_token().to_string();
    let response = client
        .send(&app, client.request("GET", "/api/whoami", None))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .any(|c| c.to_str().unwrap().contains("Max-Age=0"))
    );
    assert!(
        client
            .cookies
            .iter()
            .all(|(n, _)| n != "__Host-nail-session")
    );

    // Rotation: after the interval the response carries a new cookie.
    client.cookies = saved;
    h.clock.advance(h.auth.policy().rotate_after_secs);
    let before = client.cookies[0].1.clone();
    let response = client
        .send(&app, client.request("GET", "/api/whoami", None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let after = client
        .cookies
        .iter()
        .find(|(n, _)| n == "__Host-nail-session")
        .unwrap()
        .1
        .clone();
    assert_ne!(before, after);
    drop(browser.resolve(&h));
}

#[tokio::test]
async fn full_login_flow_over_http() {
    let (_h, app, code, mut client) = pairing_client().await;
    let stale = client.csrf.clone();
    let response = client.pair(&app, &code).await;
    // The CSRF binding moved to the new device; the response carries the
    // new token.
    let fresh = response.headers()[CSRF_HEADER].to_str().unwrap().to_owned();
    assert_ne!(Some(&fresh), stale.as_ref());
    assert!(text(response).await.contains("LoggedIn"));
    let (_, reply) = client
        .auth(&app, serde_json::json!({"type": "Logout"}))
        .await;
    assert!(matches!(
        reply,
        AuthResponse::Failed {
            reason: AuthFailure::BadRequest,
            ..
        }
    ));
    client.csrf = Some(fresh);
    let (_, reply) = client
        .auth(&app, serde_json::json!({"type": "Logout"}))
        .await;
    assert_eq!(reply, AuthResponse::LoggedOut);
    let response = client
        .send(&app, client.request("GET", "/api/whoami", None))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Password login on the paired device; logout keeps the CSRF token.
    let (_, reply) = client
        .auth(
            &app,
            serde_json::json!({"type": "Password", "user_name": "ada", "password": PASSWORD}),
        )
        .await;
    assert!(matches!(reply, AuthResponse::LoggedIn { .. }), "{reply:?}");
    let response = client
        .send(&app, client.request("GET", "/api/whoami", None))
        .await;
    assert_eq!(text(response).await, "ada");
    let (_, reply) = client
        .auth(&app, serde_json::json!({"type": "Logout"}))
        .await;
    assert_eq!(reply, AuthResponse::LoggedOut);
}

#[tokio::test]
async fn the_extractor_rejects_requests_without_the_middleware() {
    let h = Harness::new();
    let app: Router = Router::new().route(
        "/raw",
        get(|ctx: RemoteSessionCtx| async move { ctx.session.user_name }),
    );
    let _ = &h;
    let request = Request::builder().uri("/raw").body(Body::empty()).unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
