//! Fetching one remote resource without SSRF.
//!
//! Per hop (the first URL and every redirect):
//!
//! 1. the URL rules ([`crate::policy::check_url`]);
//! 2. DNS resolution through a [`Resolver`] (once per hop); **every**
//!    answer must be public ([`crate::policy::is_public_ip`]) — one bad
//!    answer refuses the hop, so a name that mixes public and private
//!    answers cannot steer the connection;
//! 3. a connection pinned to exactly the checked addresses (reqwest's
//!    resolve override, with a resolver that refuses every other name), so
//!    there is no second lookup that a rebinding DNS server could answer
//!    differently; the peer address of the response is checked again;
//! 4. redirects are never followed by the HTTP client: each `Location` is
//!    a new hop, at most [`FetchLimits::max_redirects`].
//!
//! Requests carry no cookies, no `Referer`, no credentials and a fixed
//! `User-Agent`; no system proxy is used. Responses must be `2xx` with a
//! `Content-Type` from the allow-list (raster images, fonts, CSS — no SVG,
//! no HTML) and at most [`FetchLimits::max_bytes`]; the whole fetch is
//! bounded by [`FetchLimits::timeout`].

use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{self, HeaderValue};
use url::{Host, Url};

use crate::policy::{self, UrlRefusal};

/// `User-Agent` of proxy requests (the same for every user: no version,
/// no platform).
pub const USER_AGENT: &str = "Nail-Remote-Content-Proxy";

/// What the proxy asks for.
const ACCEPT: &str = "image/avif,image/webp,image/png,image/jpeg,image/gif,image/*;q=0.8,font/*;q=0.8,text/css;q=0.5";

/// Content types the proxy passes on, and the type it answers with.
const ALLOWED_TYPES: &[(&str, &str)] = &[
    ("image/png", "image/png"),
    ("image/apng", "image/apng"),
    ("image/jpeg", "image/jpeg"),
    ("image/jpg", "image/jpeg"),
    ("image/pjpeg", "image/jpeg"),
    ("image/gif", "image/gif"),
    ("image/webp", "image/webp"),
    ("image/avif", "image/avif"),
    ("image/bmp", "image/bmp"),
    ("image/x-icon", "image/x-icon"),
    ("image/vnd.microsoft.icon", "image/x-icon"),
    ("font/woff", "font/woff"),
    ("font/woff2", "font/woff2"),
    ("font/ttf", "font/ttf"),
    ("font/otf", "font/otf"),
    ("font/sfnt", "font/sfnt"),
    ("application/font-woff", "font/woff"),
    ("application/font-woff2", "font/woff2"),
    ("application/x-font-woff", "font/woff"),
    ("application/x-font-ttf", "font/ttf"),
    ("application/x-font-otf", "font/otf"),
    ("text/css", "text/css"),
];

/// The type the proxy answers with for an upstream `Content-Type`, `None`
/// when the type is not allowed (HTML, SVG, scripts, JSON, …).
pub fn allowed_content_type(content_type: &str) -> Option<&'static str> {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    ALLOWED_TYPES
        .iter()
        .find(|(from, _)| *from == essence)
        .map(|(_, to)| *to)
}

/// Limits of one fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchLimits {
    /// Largest body accepted.
    pub max_bytes: u64,
    /// Deadline for the whole fetch, redirects included.
    pub timeout: Duration,
    /// Deadline for one TCP (+TLS) connection.
    pub connect_timeout: Duration,
    /// Deadline for one DNS lookup.
    pub dns_timeout: Duration,
    /// Redirects followed at most.
    pub max_redirects: u8,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            max_bytes: 5 * 1024 * 1024,
            timeout: Duration::from_secs(15),
            connect_timeout: Duration::from_secs(5),
            dns_timeout: Duration::from_secs(5),
            max_redirects: 5,
        }
    }
}

/// Why a fetch failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// The URL (or a redirect target) breaks the URL rules.
    #[error("refused URL: {0}")]
    Url(#[from] UrlRefusal),
    /// The name resolves to (or the connection reached) a non-public
    /// address.
    #[error("refused non-public address {0}")]
    Blocked(IpAddr),
    /// The name does not resolve.
    #[error("the host name does not resolve")]
    Resolve,
    /// More redirects than allowed, or a redirect without a usable
    /// `Location`.
    #[error("too many or invalid redirects")]
    Redirects,
    /// The server answered with a non-success status.
    #[error("upstream status {0}")]
    Status(u16),
    /// The content type is not on the allow-list.
    #[error("content type {0:?} is not allowed")]
    UnsupportedType(String),
    /// The body exceeds the size limit.
    #[error("the resource is too large")]
    TooLarge,
    /// A deadline passed.
    #[error("timed out")]
    Timeout,
    /// Connecting or reading failed.
    #[error("network error: {0}")]
    Network(String),
}

/// A fetched resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The normalized, allowed content type.
    pub content_type: &'static str,
    /// The body.
    pub body: Vec<u8>,
    /// The URL that answered (after redirects).
    pub final_url: Url,
}

/// A boxed lookup future.
pub type Lookup<'a> = Pin<Box<dyn Future<Output = io::Result<Vec<IpAddr>>> + Send + 'a>>;

/// Resolves host names to addresses. The fetcher checks every answer.
pub trait Resolver: Send + Sync + 'static {
    /// All addresses of `host` (a DNS name, never an IP literal).
    fn lookup<'a>(&'a self, host: &'a str) -> Lookup<'a>;
}

/// The operating system's resolver (`getaddrinfo`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

impl Resolver for SystemResolver {
    fn lookup<'a>(&'a self, host: &'a str) -> Lookup<'a> {
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host, 0)).await?;
            Ok(addrs.map(|a| a.ip()).collect())
        })
    }
}

/// reqwest's resolver for pinned clients: every name must come from the
/// override map; anything else fails (no fallback to system DNS).
struct RefuseResolver;

impl reqwest::dns::Resolve for RefuseResolver {
    fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async {
            Err::<reqwest::dns::Addrs, _>(
                io::Error::new(io::ErrorKind::PermissionDenied, "unpinned lookup refused").into(),
            )
        })
    }
}

/// Fetches remote resources under the rules in the module docs.
#[derive(Clone)]
pub struct Fetcher {
    tls: Arc<rustls::ClientConfig>,
    resolver: Arc<dyn Resolver>,
    limits: FetchLimits,
    #[cfg(feature = "test-support")]
    test_port: Option<u16>,
}

impl std::fmt::Debug for Fetcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fetcher")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl Fetcher {
    /// A fetcher using `tls` (a ring-based rustls client configuration
    /// with the platform verifier; the caller builds it) and the system
    /// resolver.
    pub fn new(tls: Arc<rustls::ClientConfig>) -> Self {
        Self {
            tls,
            resolver: Arc::new(SystemResolver),
            limits: FetchLimits::default(),
            #[cfg(feature = "test-support")]
            test_port: None,
        }
    }

    /// Uses another resolver (tests: scripted DNS answers).
    #[must_use]
    pub fn with_resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = resolver;
        self
    }

    /// Uses other limits.
    #[must_use]
    pub fn with_limits(mut self, limits: FetchLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The limits in use.
    pub fn limits(&self) -> FetchLimits {
        self.limits
    }

    /// **Tests only** (feature `test-support`): treats loopback addresses
    /// on exactly `port` as public, so tests can reach one mock server.
    /// Every other rule still applies.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn allow_loopback_port_for_tests(mut self, port: u16) -> Self {
        self.test_port = Some(port);
        self
    }

    fn address_allowed(&self, ip: IpAddr, port: u16) -> bool {
        if policy::is_public_ip(ip) {
            return true;
        }
        #[cfg(feature = "test-support")]
        if self.test_port == Some(port) && ip.is_loopback() {
            return true;
        }
        let _ = port;
        false
    }

    /// Fetches `url` (see the module docs).
    pub async fn fetch(&self, url: &Url) -> Result<Fetched, FetchError> {
        match tokio::time::timeout(self.limits.timeout, self.fetch_inner(url.clone())).await {
            Ok(result) => result,
            Err(_) => Err(FetchError::Timeout),
        }
    }

    async fn fetch_inner(&self, mut url: Url) -> Result<Fetched, FetchError> {
        url.set_fragment(None);
        let mut redirects = 0_u8;
        loop {
            let response = self.hop(&url).await?;
            let status = response.status();
            if status.is_redirection() {
                if redirects >= self.limits.max_redirects {
                    return Err(FetchError::Redirects);
                }
                redirects += 1;
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or(FetchError::Redirects)?;
                let mut next = url.join(location).map_err(|_| FetchError::Redirects)?;
                next.set_fragment(None);
                tracing::debug!(from = %url, to = %next, "proxy redirect");
                url = next;
                continue;
            }
            if !status.is_success() {
                return Err(FetchError::Status(status.as_u16()));
            }
            return self.read(response, url).await;
        }
    }

    /// One request to `url`, connected only to checked addresses.
    async fn hop(&self, url: &Url) -> Result<reqwest::Response, FetchError> {
        let Some(port) = url.port_or_known_default() else {
            // No port: not http(s); let the URL rules name the reason.
            policy::check_url(url)?;
            return Err(UrlRefusal::Invalid.into());
        };
        match policy::check_url(url) {
            Ok(()) => {}
            Err(UrlRefusal::Address(ip)) if self.address_allowed(ip, port) => {}
            Err(UrlRefusal::Address(ip)) => return Err(FetchError::Blocked(ip)),
            Err(refusal) => return Err(refusal.into()),
        }
        let mut builder = reqwest::Client::builder()
            .tls_backend_preconfigured((*self.tls).clone())
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .connect_timeout(self.limits.connect_timeout)
            .timeout(self.limits.timeout)
            .pool_max_idle_per_host(0)
            .user_agent(USER_AGENT)
            .dns_resolver(Arc::new(RefuseResolver));
        let allowed: Vec<IpAddr> = match url.host() {
            Some(Host::Domain(name)) => {
                let addrs = self.resolve(name, port).await?;
                let pinned: Vec<SocketAddr> =
                    addrs.iter().map(|ip| SocketAddr::new(*ip, port)).collect();
                builder = builder.resolve_to_addrs(name, &pinned);
                addrs
            }
            Some(Host::Ipv4(ip)) => vec![IpAddr::V4(ip)],
            Some(Host::Ipv6(ip)) => vec![IpAddr::V6(ip)],
            None => return Err(UrlRefusal::Invalid.into()),
        };
        let client = builder
            .build()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        let response = client
            .get(url.clone())
            .header(header::ACCEPT, HeaderValue::from_static(ACCEPT))
            .header(
                header::ACCEPT_ENCODING,
                HeaderValue::from_static("identity"),
            )
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    FetchError::Timeout
                } else {
                    FetchError::Network(e.to_string())
                }
            })?;
        // Belt and braces: the peer must be one of the checked addresses.
        if let Some(peer) = response.remote_addr() {
            let ip = canonical(peer.ip());
            if !allowed.iter().any(|a| canonical(*a) == ip) || !self.address_allowed(ip, port) {
                return Err(FetchError::Blocked(ip));
            }
        }
        Ok(response)
    }

    /// Resolves `name` once and checks every answer.
    async fn resolve(&self, name: &str, port: u16) -> Result<Vec<IpAddr>, FetchError> {
        let lookup = self.resolver.lookup(name);
        let addrs = match tokio::time::timeout(self.limits.dns_timeout, lookup).await {
            Ok(Ok(addrs)) => addrs,
            Ok(Err(_)) => return Err(FetchError::Resolve),
            Err(_) => return Err(FetchError::Timeout),
        };
        if addrs.is_empty() {
            return Err(FetchError::Resolve);
        }
        if let Some(bad) = addrs.iter().find(|ip| !self.address_allowed(**ip, port)) {
            return Err(FetchError::Blocked(*bad));
        }
        Ok(addrs)
    }

    async fn read(&self, mut response: reqwest::Response, url: Url) -> Result<Fetched, FetchError> {
        let declared = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let content_type = allowed_content_type(&declared)
            .ok_or_else(|| FetchError::UnsupportedType(declared.clone()))?;
        if response
            .content_length()
            .is_some_and(|len| len > self.limits.max_bytes)
        {
            return Err(FetchError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| FetchError::Network(e.to_string()))?
        {
            if body.len() as u64 + chunk.len() as u64 > self.limits.max_bytes {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Fetched {
            content_type,
            body,
            final_url: url,
        })
    }
}

/// IPv4-mapped IPv6 peers compare as IPv4.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allow_listed_types_pass() {
        assert_eq!(allowed_content_type("image/png"), Some("image/png"));
        assert_eq!(
            allowed_content_type("IMAGE/JPG; charset=binary"),
            Some("image/jpeg")
        );
        assert_eq!(
            allowed_content_type("text/css; charset=utf-8"),
            Some("text/css")
        );
        assert_eq!(
            allowed_content_type("application/font-woff"),
            Some("font/woff")
        );
        for refused in [
            "",
            "text/html",
            "image/svg+xml",
            "application/javascript",
            "text/javascript",
            "application/json",
            "application/xhtml+xml",
            "application/octet-stream",
            "multipart/x-mixed-replace",
            "text/plain",
            "image/png-but-not",
        ] {
            assert_eq!(allowed_content_type(refused), None, "{refused}");
        }
    }

    #[test]
    fn mapped_peers_compare_as_ipv4() {
        assert_eq!(
            canonical("::ffff:127.0.0.1".parse().unwrap()),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            canonical("::1".parse().unwrap()),
            "::1".parse::<IpAddr>().unwrap()
        );
    }
}
