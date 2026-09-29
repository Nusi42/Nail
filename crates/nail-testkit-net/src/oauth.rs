//! A mock OAuth2 authorization server (RFC 6749 authorization code grant
//! with PKCE, RFC 7636; refresh tokens) on [`axum`].
//!
//! * `GET /authorize` approves at once (the "user" is the `login_hint`, else
//!   [`MockOAuthConfig::user`]) and redirects to the loopback `redirect_uri`
//!   with `code` and `state`, or with `error=access_denied` when configured
//!   to deny. Only `http://127.0.0.1:<port>` / `http://localhost:<port>`
//!   redirect URIs and `S256` challenges are accepted.
//! * `POST /token` handles `authorization_code` (checks client, redirect URI
//!   and the PKCE verifier; codes are single-use) and `refresh_token`
//!   (optionally rotating refresh tokens). Errors use the RFC 6749 JSON
//!   shape (`invalid_grant`, `invalid_client`, …).
//!
//! A test plays the browser: fetch [`MockOAuthServer::authorize`]'s URL
//! without following redirects and deliver the `Location` to the client's
//! loopback listener, or call [`MockOAuthServer::approve`] which does both.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use sha2::{Digest as _, Sha256};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::MockNetError;

/// Configuration of a [`MockOAuthServer`].
#[derive(Debug, Clone)]
pub struct MockOAuthConfig {
    /// The registered client id.
    pub client_id: String,
    /// The client secret, if the client must authenticate (else a public
    /// client).
    pub client_secret: Option<String>,
    /// The user who "signs in" when the request has no `login_hint`.
    pub user: String,
    /// `expires_in` of issued access tokens, in seconds.
    pub access_token_lifetime: u64,
    /// Issue a new refresh token on every refresh (and invalidate the old).
    pub rotate_refresh_tokens: bool,
    /// Answer every authorization request with `error=access_denied`.
    pub deny: bool,
}

impl Default for MockOAuthConfig {
    fn default() -> Self {
        Self {
            client_id: "nail-test-client".to_owned(),
            client_secret: None,
            user: "sam@nail.example".to_owned(),
            access_token_lifetime: 3600,
            rotate_refresh_tokens: false,
            deny: false,
        }
    }
}

/// A token issued by the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedToken {
    /// The user the token belongs to.
    pub user: String,
    /// The access token.
    pub access_token: String,
    /// The refresh token, if one was issued with it.
    pub refresh_token: Option<String>,
}

/// A request received by the token endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRequest {
    /// `grant_type` form field.
    pub grant_type: String,
    /// Whether the client authenticated with HTTP Basic.
    pub basic_auth: bool,
    /// Form fields.
    pub form: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct PendingCode {
    client_id: String,
    redirect_uri: String,
    challenge: String,
    scope: String,
    user: String,
}

#[derive(Debug)]
struct Inner {
    config: Mutex<MockOAuthConfig>,
    codes: Mutex<HashMap<String, PendingCode>>,
    /// refresh token → (user, scope)
    refresh: Mutex<HashMap<String, (String, String)>>,
    issued: Mutex<Vec<IssuedToken>>,
    authorize_requests: Mutex<Vec<HashMap<String, String>>>,
    token_requests: Mutex<Vec<TokenRequest>>,
    counter: AtomicUsize,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    fn next(&self, prefix: &str) -> String {
        format!(
            "{prefix}-{}",
            self.counter.fetch_add(1, Ordering::SeqCst) + 1
        )
    }
}

/// A running mock OAuth2 server on `127.0.0.1`. Stops when dropped.
#[derive(Debug)]
pub struct MockOAuthServer {
    addr: SocketAddr,
    inner: Arc<Inner>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MockOAuthServer {
    /// Starts a server on a free loopback port.
    pub async fn start(config: MockOAuthConfig) -> Result<Self, MockNetError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let inner = Arc::new(Inner {
            config: Mutex::new(config),
            codes: Mutex::default(),
            refresh: Mutex::default(),
            issued: Mutex::default(),
            authorize_requests: Mutex::default(),
            token_requests: Mutex::default(),
            counter: AtomicUsize::new(0),
        });
        let app = Router::new()
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .with_state(Arc::clone(&inner));
        let (shutdown, mut stop) = watch::channel(false);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = stop.changed().await;
                })
                .await;
        });
        Ok(Self {
            addr,
            inner,
            shutdown,
            task,
        })
    }

    /// The listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The authorization endpoint URL.
    pub fn authorize_url(&self) -> String {
        format!("{}/authorize", self.base_url())
    }

    /// The token endpoint URL.
    pub fn token_url(&self) -> String {
        format!("{}/token", self.base_url())
    }

    /// Makes the authorization endpoint deny (or approve) from now on.
    pub fn set_deny(&self, deny: bool) {
        lock(&self.inner.config).deny = deny;
    }

    /// Plays the user's browser: requests `authorize_url` (a full URL built
    /// by the client under test) without following redirects and delivers
    /// the redirect to the loopback listener. Returns the listener's HTTP
    /// status.
    pub async fn approve(authorize_url: &str) -> Result<u16, String> {
        let location = browser_get(authorize_url)
            .await?
            .1
            .ok_or_else(|| "the authorization endpoint did not redirect".to_owned())?;
        Ok(browser_get(&location).await?.0)
    }

    /// Every token issued so far (authorization code and refresh grants).
    pub fn issued(&self) -> Vec<IssuedToken> {
        lock(&self.inner.issued).clone()
    }

    /// The user an access token was issued to, if it is known.
    pub fn user_of(&self, access_token: &str) -> Option<String> {
        lock(&self.inner.issued)
            .iter()
            .find(|t| t.access_token == access_token)
            .map(|t| t.user.clone())
    }

    /// Invalidates every refresh token (as if the user revoked access).
    pub fn revoke_all(&self) {
        lock(&self.inner.refresh).clear();
    }

    /// Query parameters of every authorization request.
    pub fn authorize_requests(&self) -> Vec<HashMap<String, String>> {
        lock(&self.inner.authorize_requests).clone()
    }

    /// Every token endpoint request.
    pub fn token_requests(&self) -> Vec<TokenRequest> {
        lock(&self.inner.token_requests).clone()
    }
}

impl Drop for MockOAuthServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

/// A minimal HTTP/1.1 GET over TCP (no redirects): (status, Location).
async fn browser_get(url: &str) -> Result<(u16, Option<String>), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    let host = parsed.host_str().ok_or("no host")?.to_owned();
    let port = parsed.port_or_known_default().ok_or("no port")?;
    let mut path = parsed.path().to_owned();
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }
    let mut tcp = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| e.to_string())?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    tcp.write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    tcp.read_to_end(&mut response)
        .await
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&response);
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("malformed HTTP response")?;
    let location = text
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("location")
                .then(|| value.trim().to_owned())
        });
    Ok((status, location))
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        Json(serde_json::json!({"error": error, "error_description": description})),
    )
        .into_response()
}

fn is_loopback_redirect(uri: &str) -> bool {
    url::Url::parse(uri).is_ok_and(|u| {
        u.scheme() == "http"
            && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
            && u.port().is_some()
    })
}

async fn authorize(
    State(inner): State<Arc<Inner>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    lock(&inner.authorize_requests).push(params.clone());
    let config = lock(&inner.config).clone();
    let get = |k: &str| params.get(k).cloned().unwrap_or_default();
    let redirect_uri = get("redirect_uri");
    if get("client_id") != config.client_id || !is_loopback_redirect(&redirect_uri) {
        // Never redirect to an unverified URI (RFC 6749 4.1.2.1).
        return (StatusCode::BAD_REQUEST, "invalid client or redirect_uri").into_response();
    }
    let mut target = url::Url::parse(&redirect_uri).expect("checked above");
    {
        let mut query = target.query_pairs_mut();
        if get("response_type") != "code" {
            query.append_pair("error", "unsupported_response_type");
        } else if !params.contains_key("code_challenge") || get("code_challenge_method") != "S256" {
            query.append_pair("error", "invalid_request");
            query.append_pair("error_description", "PKCE with S256 is required");
        } else if config.deny {
            query.append_pair("error", "access_denied");
        } else {
            let code = inner.next("code");
            let user = params
                .get("login_hint")
                .cloned()
                .unwrap_or(config.user.clone());
            lock(&inner.codes).insert(
                code.clone(),
                PendingCode {
                    client_id: config.client_id.clone(),
                    redirect_uri: redirect_uri.clone(),
                    challenge: get("code_challenge"),
                    scope: get("scope"),
                    user,
                },
            );
            query.append_pair("code", &code);
        }
        if let Some(state) = params.get("state") {
            query.append_pair("state", state);
        }
    }
    (StatusCode::FOUND, [(header::LOCATION, target.to_string())]).into_response()
}

/// Client authentication (RFC 6749 2.3.1): HTTP Basic or form fields.
fn client_ok(
    config: &MockOAuthConfig,
    headers: &HeaderMap,
    form: &HashMap<String, String>,
) -> (bool, bool) {
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b| STANDARD.decode(b).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| {
            let (id, secret) = s.split_once(':')?;
            let decode = |x: &str| {
                url::form_urlencoded::parse(format!("x={x}").as_bytes())
                    .next()
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default()
            };
            Some((decode(id), decode(secret)))
        });
    let (id, secret) = match &basic {
        Some((id, secret)) => (Some(id.clone()), Some(secret.clone())),
        None => (
            form.get("client_id").cloned(),
            form.get("client_secret").cloned(),
        ),
    };
    let id_ok = id.as_deref() == Some(config.client_id.as_str());
    let secret_ok = match &config.client_secret {
        Some(expected) => secret.as_deref() == Some(expected.as_str()),
        None => secret.as_deref().is_none_or(str::is_empty),
    };
    (id_ok && secret_ok, basic.is_some())
}

fn issue(
    inner: &Inner,
    config: &MockOAuthConfig,
    user: &str,
    scope: &str,
    refresh: Option<String>,
) -> Response {
    let access_token = inner.next("access");
    lock(&inner.issued).push(IssuedToken {
        user: user.to_owned(),
        access_token: access_token.clone(),
        refresh_token: refresh.clone(),
    });
    let mut body = serde_json::json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": config.access_token_lifetime,
        "scope": scope,
    });
    if let Some(refresh) = refresh {
        lock(&inner.refresh).insert(refresh.clone(), (user.to_owned(), scope.to_owned()));
        body["refresh_token"] = serde_json::Value::String(refresh);
    }
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
        .into_response()
}

async fn token(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let config = lock(&inner.config).clone();
    let (client_ok, basic_auth) = client_ok(&config, &headers, &form);
    let grant_type = form.get("grant_type").cloned().unwrap_or_default();
    lock(&inner.token_requests).push(TokenRequest {
        grant_type: grant_type.clone(),
        basic_auth,
        form: form.clone(),
    });
    if !client_ok {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication failed",
        );
    }
    match grant_type.as_str() {
        "authorization_code" => {
            let code = form.get("code").cloned().unwrap_or_default();
            let Some(pending) = lock(&inner.codes).remove(&code) else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "unknown or used code",
                );
            };
            if pending.client_id != config.client_id
                || form.get("redirect_uri") != Some(&pending.redirect_uri)
            {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "redirect_uri mismatch",
                );
            }
            let verifier = form.get("code_verifier").cloned().unwrap_or_default();
            let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            if computed != pending.challenge {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "PKCE verification failed",
                );
            }
            let refresh = inner.next("refresh");
            issue(
                &inner,
                &config,
                &pending.user,
                &pending.scope,
                Some(refresh),
            )
        }
        "refresh_token" => {
            let token = form.get("refresh_token").cloned().unwrap_or_default();
            let known = lock(&inner.refresh).get(&token).cloned();
            let Some((user, scope)) = known else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "refresh token revoked or unknown",
                );
            };
            let refresh = if config.rotate_refresh_tokens {
                lock(&inner.refresh).remove(&token);
                Some(inner.next("refresh"))
            } else {
                None
            };
            issue(&inner, &config, &user, &scope, refresh)
        }
        _ => oauth_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "unsupported grant type",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_redirects_only() {
        assert!(is_loopback_redirect("http://127.0.0.1:4000/cb"));
        assert!(is_loopback_redirect("http://localhost:4000/"));
        assert!(!is_loopback_redirect("http://127.0.0.1/cb"));
        assert!(!is_loopback_redirect("https://evil.example:4000/"));
    }

    #[tokio::test]
    async fn denies_foreign_redirects_without_redirecting() {
        let server = MockOAuthServer::start(MockOAuthConfig::default())
            .await
            .unwrap();
        let url = format!(
            "{}?response_type=code&client_id=nail-test-client&redirect_uri=https%3A%2F%2Fevil.example%2F",
            server.authorize_url()
        );
        let (status, location) = browser_get(&url).await.unwrap();
        assert_eq!((status, location), (400, None));
    }

    #[tokio::test]
    async fn code_grant_checks_pkce_and_is_single_use() {
        let server = MockOAuthServer::start(MockOAuthConfig::default())
            .await
            .unwrap();
        // Stand-in for the client's loopback listener.
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let redirect = format!(
            "http://127.0.0.1:{}/cb",
            listener.local_addr().unwrap().port()
        );
        let verifier = "a".repeat(43);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = url::Url::parse(&server.authorize_url()).unwrap();
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", "nail-test-client")
            .append_pair("redirect_uri", &redirect)
            .append_pair("state", "xyz")
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("login_hint", "ada@nail.example");
        let (status, location) = browser_get(url.as_str()).await.unwrap();
        assert_eq!(status, 302);
        let location = url::Url::parse(&location.unwrap()).unwrap();
        let query: HashMap<_, _> = location.query_pairs().into_owned().collect();
        assert_eq!(query["state"], "xyz");
        let code = query["code"].clone();

        let form = |verifier: &str| {
            [
                ("grant_type", "authorization_code".to_owned()),
                ("code", code.clone()),
                ("redirect_uri", redirect.clone()),
                ("client_id", "nail-test-client".to_owned()),
                ("code_verifier", verifier.to_owned()),
            ]
        };
        let post = |body: String| {
            let addr = server.addr();
            async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
                let req = format!(
                    "POST /token HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                tcp.write_all(req.as_bytes()).await.unwrap();
                let mut out = String::new();
                tcp.read_to_string(&mut out).await.unwrap();
                out
            }
        };
        let encode = |pairs: &[(&str, String)]| {
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs.iter().map(|(k, v)| (*k, v.as_str())))
                .finish()
        };
        // A wrong verifier consumes the code.
        let bad = post(encode(&form(&"b".repeat(43)))).await;
        assert!(bad.contains("invalid_grant"), "{bad}");
        let reused = post(encode(&form(&verifier))).await;
        assert!(reused.contains("invalid_grant"), "{reused}");
        assert!(server.issued().is_empty());
        assert_eq!(server.token_requests().len(), 2);
    }
}
