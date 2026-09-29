//! Serving a token: verify, optionally re-check with the core, fetch.
//! Shared by `nail-server` (`GET /api/proxy/<token>`) and the desktop
//! shell's `nail-proxy:` protocol.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use url::Url;

use crate::fetch::{FetchError, Fetcher};
use crate::token::{Purpose, Signer, TokenError};

/// Headers every proxied response carries (besides `Content-Type` and
/// `Cache-Control`): the body is inert data, never a document.
pub const RESPONSE_HEADERS: &[(&str, &str)] = &[
    ("x-content-type-options", "nosniff"),
    (
        "content-security-policy",
        "default-src 'none'; style-src 'unsafe-inline'; sandbox",
    ),
    ("referrer-policy", "no-referrer"),
    // Loaded by the sandboxed (opaque-origin) mail document under COEP.
    ("cross-origin-resource-policy", "cross-origin"),
    ("content-disposition", "inline"),
];

/// Longest time a browser may cache a proxied resource.
const MAX_CACHE_SECS: u64 = 60 * 60;

/// A boxed check future.
pub type Check<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// A second opinion before fetching: the core confirms that the message is
/// still allowed to load remote content (the user may have removed the
/// allow-list entry since the token was minted) and that it references
/// `url`.
pub trait TargetCheck: Send + Sync + 'static {
    /// Whether `message_id` may load `url` now.
    fn allows<'a>(&'a self, message_id: &'a str, url: &'a Url) -> Check<'a>;
}

/// Why a proxy request failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProxyError {
    /// The token is invalid, for another purpose or expired.
    #[error("{0}")]
    Token(#[from] TokenError),
    /// The core no longer allows this message's remote content.
    #[error("remote content is not allowed for this message")]
    Denied,
    /// Fetching failed or was refused.
    #[error("{0}")]
    Fetch(#[from] FetchError),
}

impl ProxyError {
    /// The HTTP status a server answers with. Token problems are all `404`
    /// (nothing to learn by probing); refused destinations `403`.
    pub fn status(&self) -> u16 {
        match self {
            Self::Token(_) => 404,
            Self::Denied => 403,
            Self::Fetch(err) => match err {
                FetchError::Url(_) | FetchError::Blocked(_) => 403,
                FetchError::UnsupportedType(_) => 415,
                FetchError::TooLarge => 413,
                FetchError::Timeout => 504,
                FetchError::Resolve
                | FetchError::Redirects
                | FetchError::Status(_)
                | FetchError::Network(_) => 502,
            },
        }
    }
}

/// A resource ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyResponse {
    /// `Content-Type` (from the allow-list).
    pub content_type: &'static str,
    /// The body.
    pub body: Vec<u8>,
    /// `Cache-Control` value (`private`, bounded by the token's life).
    pub cache_control: String,
}

/// The remote-content proxy: a [`Signer`] (for verifying), a [`Fetcher`]
/// and an optional [`TargetCheck`].
#[derive(Clone)]
pub struct Proxy {
    signer: Signer,
    fetcher: Fetcher,
    check: Option<Arc<dyn TargetCheck>>,
}

impl std::fmt::Debug for Proxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proxy")
            .field("fetcher", &self.fetcher)
            .field("check", &self.check.is_some())
            .finish_non_exhaustive()
    }
}

impl Proxy {
    /// A proxy verifying with `signer` and fetching with `fetcher`.
    pub fn new(signer: Signer, fetcher: Fetcher) -> Self {
        Self {
            signer,
            fetcher,
            check: None,
        }
    }

    /// Adds the core's [`TargetCheck`].
    #[must_use]
    pub fn with_check(mut self, check: Arc<dyn TargetCheck>) -> Self {
        self.check = Some(check);
        self
    }

    /// The signer (the server also verifies render tokens with it).
    pub fn signer(&self) -> &Signer {
        &self.signer
    }

    /// Serves `token` (with or without the `nail-proxy:` prefix).
    pub async fn serve(&self, token: &str) -> Result<ProxyResponse, ProxyError> {
        let grant = self.signer.verify(token, Purpose::Resource)?;
        let url = grant.url.ok_or(TokenError::Invalid)?;
        if let Some(check) = &self.check
            && !check.allows(&grant.message_id, &url).await
        {
            return Err(ProxyError::Denied);
        }
        let fetched = self.fetcher.fetch(&url).await.inspect_err(|err| {
            tracing::info!(message = %grant.message_id, %err, "remote content not loaded");
        })?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let max_age = grant.expires.saturating_sub(now).min(MAX_CACHE_SECS);
        Ok(ProxyResponse {
            content_type: fetched.content_type,
            body: fetched.body,
            cache_control: format!("private, max-age={max_age}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::FetchError;
    use crate::policy::UrlRefusal;

    #[test]
    fn statuses() {
        assert_eq!(ProxyError::Token(TokenError::Expired).status(), 404);
        assert_eq!(ProxyError::Token(TokenError::Invalid).status(), 404);
        assert_eq!(ProxyError::Denied.status(), 403);
        assert_eq!(
            ProxyError::Fetch(FetchError::Blocked("127.0.0.1".parse().unwrap())).status(),
            403
        );
        assert_eq!(
            ProxyError::Fetch(FetchError::Url(UrlRefusal::Scheme)).status(),
            403
        );
        assert_eq!(ProxyError::Fetch(FetchError::TooLarge).status(), 413);
        assert_eq!(
            ProxyError::Fetch(FetchError::UnsupportedType("text/html".into())).status(),
            415
        );
        assert_eq!(ProxyError::Fetch(FetchError::Timeout).status(), 504);
        assert_eq!(ProxyError::Fetch(FetchError::Status(500)).status(), 502);
    }

    #[test]
    fn response_headers_make_the_body_inert() {
        let get = |name: &str| {
            RESPONSE_HEADERS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| *v)
        };
        assert_eq!(get("x-content-type-options"), Some("nosniff"));
        assert!(get("content-security-policy").unwrap().contains("sandbox"));
        assert!(
            get("content-security-policy")
                .unwrap()
                .starts_with("default-src 'none'")
        );
    }
}
