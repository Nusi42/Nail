//! The request loop of the OAuth2 JSON API clients (Gmail, Microsoft
//! Graph): the base URL policy, a bearer token with one forced refresh on
//! `401`, inline retries of broken connections, throttling and server
//! errors per the [`RetryPolicy`] (or the server's `Retry-After`), bounded
//! answers, and the client's own error type for everything else.

use reqwest::Method;
use reqwest::header::HeaderMap;
use url::Url;

use crate::http::{
    self, ApiAuth, Body, Retries, RetryPolicy, TransportFailure, is_allowed_url, same_origin,
};

/// One request of an API client.
#[derive(Debug, Clone)]
pub struct ApiCall {
    /// The method.
    pub method: Method,
    /// The URL (on the API's origin).
    pub url: Url,
    /// The body.
    pub body: Body,
    /// Extra headers (`Prefer`, `If-Match`).
    pub headers: Vec<(String, String)>,
    /// Safe to repeat after a server error or a broken connection (reads,
    /// absolute writes); sends and creates are not.
    pub idempotent: bool,
    /// What is attempted (for errors and logs).
    pub what: String,
}

/// A successful (`2xx`) answer.
#[derive(Debug, Clone)]
pub struct ApiReply {
    /// The status.
    pub status: u16,
    /// The headers.
    pub headers: HeaderMap,
    /// The body.
    pub body: Vec<u8>,
}

/// Whether a failed answer may be retried inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    /// Not at all.
    Never,
    /// Only an idempotent call (a server error: the call may have run).
    IfIdempotent,
    /// Any call (throttling: the call was not processed).
    Always,
}

/// The error type of an API client.
pub trait ApiFailure: TransportFailure {
    /// The service's name in logs (`Gmail`, `Graph`).
    const SERVICE: &'static str;
    /// The URL is not allowed (scheme, host or another origin).
    fn insecure_url(url: String) -> Self;
    /// Getting an access token failed.
    fn token(error: nail_api::engine::EngineError) -> Self;
    /// The server or the client broke the protocol.
    fn protocol(message: String) -> Self;
    /// Whether the failed answer `status` with `body` may be retried.
    fn retry_class(status: u16, body: &[u8]) -> RetryClass;
    /// The error for a failed answer of `what`.
    fn from_answer(status: u16, retry_after: Option<u32>, body: &[u8], what: &str) -> Self;
}

/// Implements [`ApiFailure`] for a client error with the variants
/// `InsecureUrl(String)`, `Token(Box<EngineError>)` and
/// `Protocol(String)`; `$retry_class` is the client's retry rule, and a
/// failed answer becomes `Self::from_status(status, retry_after,
/// Option<&$body>, what)` with the service's JSON error body.
#[macro_export]
macro_rules! api_failure {
    ($ty:ident, $service:literal, $retry_class:expr, $body:ty) => {
        impl $crate::api::ApiFailure for $ty {
            const SERVICE: &'static str = $service;

            fn insecure_url(url: String) -> Self {
                Self::InsecureUrl(url)
            }

            fn token(error: $crate::http::__EngineError) -> Self {
                Self::Token(Box::new(error))
            }

            fn protocol(message: String) -> Self {
                Self::Protocol(message)
            }

            fn retry_class(status: u16, body: &[u8]) -> $crate::api::RetryClass {
                $retry_class(status, body)
            }

            fn from_answer(status: u16, retry_after: Option<u32>, body: &[u8], what: &str) -> Self {
                let error_body: Option<$body> = $crate::http::__serde_json::from_slice(body).ok();
                Self::from_status(status, retry_after, error_body.as_ref(), what)
            }
        }
    };
}

/// The constructors and accessors of a service client that wraps an
/// [`ApiClient`] in its field `api`: `with_http`, `with_retry`,
/// `retry_policy`, `api_base`, `auth`.
#[macro_export]
macro_rules! api_client_wrapper {
    ($ty:ident, $error:ty, $max_answer:expr) => {
        impl $ty {
            /// A client on an existing [`reqwest::Client`] (it must not
            /// follow redirects; build it with `nail_auth::http_client_builder`)
            /// for the API base URL `api_base`.
            pub fn with_http(
                http: reqwest::Client,
                api_base: &str,
                auth: $crate::http::ApiAuth,
            ) -> Result<Self, $error> {
                Ok(Self {
                    api: $crate::api::ApiClient::new::<$error>(http, api_base, auth, $max_answer)?,
                })
            }

            /// Replaces the retry policy.
            #[must_use]
            pub fn with_retry(self, retry: $crate::http::RetryPolicy) -> Self {
                Self {
                    api: self.api.with_retry(retry),
                }
            }

            /// The retry policy.
            pub fn retry_policy(&self) -> $crate::http::RetryPolicy {
                self.api.retry_policy()
            }

            /// The API base URL.
            pub fn api_base(&self) -> &url::Url {
                self.api.base()
            }

            /// The sign-in.
            pub fn auth(&self) -> &$crate::http::ApiAuth {
                self.api.auth()
            }
        }
    };
}

/// An OAuth2 JSON API client below one base URL.
#[derive(Debug, Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    base: Url,
    auth: ApiAuth,
    retry: RetryPolicy,
    max_answer: usize,
}

impl ApiClient {
    /// A client on `http` (which must not follow redirects) for the API
    /// base URL `api_base` (`https:`, or `http:` to a loopback host), with
    /// answers of at most `max_answer` bytes.
    pub fn new<E: ApiFailure>(
        http: reqwest::Client,
        api_base: &str,
        auth: ApiAuth,
        max_answer: usize,
    ) -> Result<Self, E> {
        let base = Url::parse(api_base).map_err(|_| E::insecure_url(api_base.to_owned()))?;
        if !is_allowed_url(&base) || base.host_str().is_none_or(str::is_empty) {
            return Err(E::insecure_url(api_base.to_owned()));
        }
        Ok(Self {
            http,
            base: http::normalized_base(base),
            auth,
            retry: RetryPolicy::default(),
            max_answer,
        })
    }

    /// Replaces the retry policy.
    #[must_use]
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// The retry policy.
    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        self.retry
    }

    /// The API base URL.
    #[must_use]
    pub fn base(&self) -> &Url {
        &self.base
    }

    /// The sign-in.
    #[must_use]
    pub fn auth(&self) -> &ApiAuth {
        &self.auth
    }

    /// The HTTP client.
    #[must_use]
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Whether `url` is on the API's origin (the bearer token never goes
    /// anywhere else).
    #[must_use]
    pub fn same_origin(&self, url: &Url) -> bool {
        same_origin(url, &self.base)
    }

    /// The `Authorization` value (`force_refresh` after a refused token).
    pub async fn authorization<E: ApiFailure>(&self, force_refresh: bool) -> Result<String, E> {
        self.auth
            .authorization(force_refresh)
            .await
            .map_err(E::token)
    }

    /// Sends `call` with `headers`: one forced token refresh on `401`,
    /// inline retries per the [`RetryPolicy`]. Non-2xx answers become `E`s.
    pub async fn execute<E: ApiFailure>(
        &self,
        call: &ApiCall,
        headers: &[(String, String)],
    ) -> Result<ApiReply, E> {
        if !is_allowed_url(&call.url) || !self.same_origin(&call.url) {
            return Err(E::insecure_url(call.url.to_string()));
        }
        let mut refreshed = false;
        let mut tries = Retries::new(self.retry);
        loop {
            let reply = match self.send_once::<E>(call, headers, refreshed).await {
                Ok(reply) => reply,
                Err(e) if e.is_transport() && call.idempotent && tries.backoff().await => continue,
                Err(e) => return Err(e),
            };
            let status = reply.status;
            if (200..300).contains(&status) {
                return Ok(reply);
            }
            if status == 401 && !refreshed && self.auth.can_refresh() {
                tracing::debug!(what = %call.what, "{} refused the token, refreshing it", E::SERVICE);
                refreshed = true;
                continue;
            }
            let retry_after = http::retry_after_secs(&reply.headers);
            let may_retry = match E::retry_class(status, &reply.body) {
                RetryClass::Never => false,
                RetryClass::IfIdempotent => call.idempotent,
                RetryClass::Always => true,
            };
            if may_retry && let Some(wait) = tries.wait(retry_after) {
                tracing::debug!(what = %call.what, status, ?wait, "{} asks to retry", E::SERVICE);
                tries.sleep(wait).await;
                continue;
            }
            return Err(E::from_answer(status, retry_after, &reply.body, &call.what));
        }
    }

    async fn send_once<E: ApiFailure>(
        &self,
        call: &ApiCall,
        headers: &[(String, String)],
        force_refresh: bool,
    ) -> Result<ApiReply, E> {
        let authorization = self.authorization::<E>(force_refresh).await?;
        let mut request = http::json_request(
            self.http.request(call.method.clone(), call.url.clone()),
            &authorization,
        )
        .ok_or_else(|| E::protocol("the token is not valid header text".into()))?;
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let mut response = call
            .body
            .apply(request, &call.method)
            .send()
            .await
            .map_err(|e| E::from(http::transport_error(e)))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = http::read_limited(&mut response, self.max_answer)
            .await
            .map_err(|e| E::from_body(e, &format!("answer to {}", call.what)))?;
        Ok(ApiReply {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum TestError {
        Timeout,
        Tls(String),
        Network(String),
        TooLarge(String),
        Insecure(String),
        Token,
        Protocol(String),
        Status(u16, Option<u32>),
    }

    crate::transport_errors!(TestError);

    impl ApiFailure for TestError {
        const SERVICE: &'static str = "Test";
        fn insecure_url(url: String) -> Self {
            Self::Insecure(url)
        }
        fn token(_: nail_api::engine::EngineError) -> Self {
            Self::Token
        }
        fn protocol(message: String) -> Self {
            Self::Protocol(message)
        }
        fn retry_class(status: u16, _: &[u8]) -> RetryClass {
            match status {
                429 => RetryClass::Always,
                500..=599 => RetryClass::IfIdempotent,
                _ => RetryClass::Never,
            }
        }
        fn from_answer(status: u16, retry_after: Option<u32>, _: &[u8], _: &str) -> Self {
            Self::Status(status, retry_after)
        }
    }

    /// A loopback server answering each connection with the next of
    /// `answers` (raw HTTP/1.1 heads + bodies); counts the requests.
    async fn server(answers: Vec<&'static str>) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        tokio::spawn(async move {
            for answer in answers {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0u8; 4096];
                drop(socket.read(&mut buf).await);
                seen.fetch_add(1, Ordering::SeqCst);
                drop(socket.write_all(answer.as_bytes()).await);
                drop(socket.shutdown().await);
            }
        });
        (format!("http://{addr}/v1"), count)
    }

    fn client(base: &str) -> ApiClient {
        let tls = crate::tls::client_config(&crate::tls::TlsTrust::WebPkiRoots).unwrap();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .pool_max_idle_per_host(0)
            .build()
            .unwrap();
        ApiClient::new::<TestError>(http, base, ApiAuth::Bearer("t".into()), 64)
            .unwrap()
            .with_retry(RetryPolicy::fast())
    }

    fn call(base: &str, idempotent: bool) -> ApiCall {
        ApiCall {
            method: Method::GET,
            url: Url::parse(&format!("{base}/me")).unwrap(),
            body: Body::None,
            headers: Vec::new(),
            idempotent,
            what: "GET /me".into(),
        }
    }

    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    const BUSY: &str =
        "HTTP/1.1 503 Busy\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const THROTTLED: &str =
        "HTTP/1.1 429 Slow\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    #[tokio::test]
    async fn server_errors_are_retried_only_for_idempotent_calls() {
        let (base, count) = server(vec![BUSY, OK]).await;
        let reply: ApiReply = client(&base)
            .execute::<TestError>(&call(&base, true), &[])
            .await
            .unwrap();
        assert_eq!((reply.status, reply.body.as_slice()), (200, &b"{}"[..]));
        assert_eq!(count.load(Ordering::SeqCst), 2);

        let (base, count) = server(vec![BUSY, OK]).await;
        let error = client(&base)
            .execute::<TestError>(&call(&base, false), &[])
            .await
            .unwrap_err();
        assert_eq!(error, TestError::Status(503, Some(0)));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn throttling_is_retried_for_any_call() {
        let (base, count) = server(vec![THROTTLED, OK]).await;
        let reply = client(&base)
            .execute::<TestError>(&call(&base, false), &[])
            .await
            .unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn urls_and_limits() {
        let base = "http://127.0.0.1:1/v1";
        assert!(matches!(
            ApiClient::new::<TestError>(
                client(base).http().clone(),
                "http://example.org/",
                ApiAuth::Bearer(String::new()),
                1
            ),
            Err(TestError::Insecure(_))
        ));
        let foreign = ApiCall {
            url: Url::parse("http://127.0.0.1:2/v1/me").unwrap(),
            ..call(base, true)
        };
        assert!(matches!(
            client(base).execute::<TestError>(&foreign, &[]).await,
            Err(TestError::Insecure(_))
        ));
        let (base, _) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n",
        ])
        .await;
        assert!(matches!(
            client(&base)
                .execute::<TestError>(&call(&base, true), &[])
                .await,
            Err(TestError::TooLarge(_))
        ));
        let c = client("http://127.0.0.1:1/v1/");
        assert_eq!(c.base().as_str(), "http://127.0.0.1:1/v1");
        assert_eq!(c.retry_policy(), RetryPolicy::fast());
        assert!(!c.auth().can_refresh());
        let _: &reqwest::Client = c.http();
        assert_eq!(
            c.authorization::<TestError>(false).await.unwrap(),
            "Bearer t"
        );
    }
}
