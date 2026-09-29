//! The OAuth2 authorization code flow for installed apps (RFC 8252): PKCE
//! (S256), a loopback redirect listener on `127.0.0.1:<random port>`, the
//! `state` check, then the code exchange.
//!
//! [`start_flow`] returns the authorization URL at once (the core opens it
//! in the browser or hands it to the UI) and a [`PendingFlow`] that finishes
//! when the browser comes back, the user cancels, or the timeout passes.
//!
//! The listener serves exactly one HTTP request per connection and ignores
//! requests for other paths and requests with a wrong `state` (so another
//! local page cannot end or hijack the flow). The page shown in the browser
//! afterwards comes from the caller ([`CallbackPages`]), because it is
//! user-visible text that must be localized.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::client::{OAuthClient, TokenSet};
use crate::error::AuthError;

/// The HTML pages the browser shows after the redirect (localized by the
/// caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackPages {
    /// Shown when the sign-in succeeded ("You can close this window").
    pub success_html: String,
    /// Shown when the provider reported an error or the user declined.
    pub failure_html: String,
}

/// Options of [`start_flow`].
#[derive(Debug, Clone)]
pub struct FlowOptions {
    /// Address to pre-fill on the provider's sign-in page.
    pub login_hint: Option<String>,
    /// How long to wait for the browser (the protocol says 10 minutes).
    pub timeout: Duration,
    /// Browser pages after the redirect.
    pub pages: CallbackPages,
}

impl FlowOptions {
    /// Default wait for the user.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

    /// Options with the default timeout.
    pub fn new(pages: CallbackPages) -> Self {
        Self {
            login_hint: None,
            timeout: Self::DEFAULT_TIMEOUT,
            pages,
        }
    }
}

/// Cancels a running flow; cheap to clone.
#[derive(Debug, Clone)]
pub struct FlowCanceller(Arc<watch::Sender<bool>>);

impl FlowCanceller {
    /// Cancels the flow: it finishes with [`AuthError::Cancelled`] and the
    /// listener closes.
    pub fn cancel(&self) {
        let _ = self.0.send(true);
    }
}

/// A started flow.
pub struct PendingFlow {
    authorize_url: String,
    redirect_uri: String,
    canceller: FlowCanceller,
    task: Option<JoinHandle<Result<TokenSet, AuthError>>>,
}

impl fmt::Debug for PendingFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingFlow")
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

impl PendingFlow {
    /// The provider's sign-in page to open in the browser.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    /// The loopback redirect URI (`http://127.0.0.1:<port>/`).
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// A handle to cancel the flow from elsewhere.
    pub fn canceller(&self) -> FlowCanceller {
        self.canceller.clone()
    }

    /// Waits for the outcome.
    pub async fn finish(mut self) -> Result<TokenSet, AuthError> {
        let Some(task) = self.task.take() else {
            return Err(AuthError::Cancelled);
        };
        match task.await {
            Ok(result) => result,
            Err(e) if e.is_cancelled() => Err(AuthError::Cancelled),
            Err(e) => Err(AuthError::Protocol(format!("sign-in task failed: {e}"))),
        }
    }
}

impl Drop for PendingFlow {
    fn drop(&mut self) {
        // A dropped flow must not keep the port open.
        self.canceller.cancel();
    }
}

/// Starts a flow for `client`: binds the loopback listener and builds the
/// authorization URL.
pub async fn start_flow(
    client: OAuthClient,
    options: FlowOptions,
) -> Result<PendingFlow, AuthError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| AuthError::Protocol(format!("cannot open the loopback listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AuthError::Protocol(e.to_string()))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/");
    let (url, state, verifier) =
        client.authorize_url(&redirect_uri, options.login_hint.as_deref())?;
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    let redirect = redirect_uri.clone();
    let provider = client.spec().id.clone();
    let task = tokio::spawn(async move {
        let wait = async {
            let code = wait_for_code(&listener, state.secret(), &options.pages).await?;
            drop(listener);
            tracing::debug!(%provider, "OAuth2 authorization code received; exchanging it");
            client.exchange_code(code, verifier, &redirect).await
        };
        tokio::select! {
            result = wait => result,
            _ = cancel_rx.wait_for(|c| *c) => Err(AuthError::Cancelled),
            () = tokio::time::sleep(options.timeout) => Err(AuthError::Timeout),
        }
    });
    Ok(PendingFlow {
        authorize_url: url.to_string(),
        redirect_uri,
        canceller: FlowCanceller(Arc::new(cancel_tx)),
        task: Some(task),
    })
}

/// What one request to the listener carried.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Callback {
    /// Not for us (other path, wrong or missing state): keep waiting.
    Ignore,
    /// The authorization code.
    Code(String),
    /// The provider's `error` (e.g. `access_denied`).
    Error(String),
}

/// Parses the request line of a callback request.
pub(crate) fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Callback::Ignore;
    };
    let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Callback::Ignore;
    };
    if url.path() != "/" {
        return Callback::Ignore;
    }
    let mut state = None;
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "state" => state = Some(value.into_owned()),
            "code" => code = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    // Constant-time enough: the state is random and single-use.
    if state.as_deref() != Some(expected_state) {
        return Callback::Ignore;
    }
    match (code, error) {
        (_, Some(error)) => Callback::Error(error),
        (Some(code), None) if !code.is_empty() => Callback::Code(code),
        _ => Callback::Ignore,
    }
}

async fn respond(stream: &mut TcpStream, status: &str, html: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// Reads the request head (up to 16 KiB, 10 s).
async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let read = async {
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.windows(2).any(|w| w == b"\n\n") {
                return Some(());
            }
            if buf.len() > 16 * 1024 {
                return None;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .ok()??;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

async fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    pages: &CallbackPages,
) -> Result<String, AuthError> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|e| AuthError::Protocol(format!("loopback listener failed: {e}")))?;
        let Some(head) = read_head(&mut stream).await else {
            continue;
        };
        let request_line = head.lines().next().unwrap_or_default();
        match parse_callback(request_line, state) {
            Callback::Ignore => respond(&mut stream, "404 Not Found", "").await,
            Callback::Code(code) => {
                respond(&mut stream, "200 OK", &pages.success_html).await;
                return Ok(code);
            }
            Callback::Error(error) => {
                respond(&mut stream, "200 OK", &pages.failure_html).await;
                return Err(if error == "access_denied" {
                    AuthError::Cancelled
                } else {
                    AuthError::Refused(error)
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_parsing() {
        assert_eq!(
            parse_callback("GET /?code=abc&state=s1 HTTP/1.1", "s1"),
            Callback::Code("abc".into())
        );
        assert_eq!(
            parse_callback("GET /?code=abc&state=s2 HTTP/1.1", "s1"),
            Callback::Ignore
        );
        assert_eq!(
            parse_callback("GET /?code=abc HTTP/1.1", "s1"),
            Callback::Ignore
        );
        assert_eq!(
            parse_callback("GET /favicon.ico HTTP/1.1", "s1"),
            Callback::Ignore
        );
        assert_eq!(
            parse_callback("POST /?code=abc&state=s1 HTTP/1.1", "s1"),
            Callback::Ignore
        );
        assert_eq!(
            parse_callback("GET /?error=access_denied&state=s1 HTTP/1.1", "s1"),
            Callback::Error("access_denied".into())
        );
        assert_eq!(
            parse_callback("GET /?code=a%2Fb&state=s%201 HTTP/1.1", "s 1"),
            Callback::Code("a/b".into())
        );
        assert_eq!(parse_callback("", "s1"), Callback::Ignore);
    }
}
