//! `nail.http.fetch` for plug-ins (M2d WP64, ADR 0055): the core executes
//! the requests the permission gate let through (`nail-plugins` checked the
//! first URL, the method and the headers against the plug-in's
//! `network:<host>` permissions) without SSRF, like `nail-proxy`:
//!
//! 1. per hop (the first URL and every redirect) the URL rules of the gate:
//!    `https:` on the default port, or `http:` / any port for loopback
//!    hosts; no credentials; the host matches a granted pattern
//!    ([`nail_plugins::gate::host_allowed`]);
//! 2. an IP literal is reached only when a pattern names exactly that
//!    literal (a plug-in declared `network:127.0.0.1`); a DNS name is
//!    resolved once and **every** answer must be public
//!    ([`nail_proxy::is_public_ip`]), so names never lead to loopback or
//!    private networks;
//! 3. the connection is pinned to the checked addresses (no second
//!    lookup), and the peer is checked again;
//! 4. redirects are hops (at most [`MAX_REDIRECTS`]); `303` (and `301` /
//!    `302` after a `POST`) continue as `GET` without a body;
//! 5. no cookies, no `Referer`, no system proxy, a fixed `User-Agent`; the
//!    body is read up to `max_response_bytes` (`too_large` beyond) and the
//!    whole request is bounded by its `timeout`.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};

use nail_api::{ApiError, ErrorCode};
use nail_plugins::gate::host_allowed;
use nail_plugins::{HttpRequest, HttpResponse};
use nail_proxy::{Resolver, SystemResolver, is_public_ip};
use reqwest::header::{HeaderName, HeaderValue};
use url::{Host, Url};

/// Redirects followed per request.
pub(crate) const MAX_REDIRECTS: u8 = 5;

/// `User-Agent` of plug-in requests.
pub(crate) const USER_AGENT: &str = "Nail-Plugin";

/// Executes plug-in HTTP requests (see the module docs).
#[derive(Clone)]
pub(crate) struct PluginHttp {
    tls: Arc<OnceLock<Option<Arc<rustls::ClientConfig>>>>,
    resolver: Arc<dyn Resolver>,
}

impl std::fmt::Debug for PluginHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHttp").finish_non_exhaustive()
    }
}

impl Default for PluginHttp {
    fn default() -> Self {
        Self {
            tls: Arc::new(OnceLock::new()),
            resolver: Arc::new(SystemResolver),
        }
    }
}

fn network(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Network, message)
}

fn refused(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

fn is_loopback_host(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => ip.is_loopback(),
    }
}

/// The host as a permission pattern compares it.
fn host_text(host: &Host<&str>) -> String {
    match host {
        Host::Domain(name) => name.to_ascii_lowercase(),
        Host::Ipv4(ip) => ip.to_string(),
        Host::Ipv6(ip) => ip.to_string(),
    }
}

/// The URL rules of one hop.
pub(crate) fn check_hop(url: &Url, allowed_hosts: &[String]) -> Result<(), ApiError> {
    let Some(host) = url.host() else {
        return Err(refused(format!("{url} has no host")));
    };
    if !url.username().is_empty() || url.password().is_some() {
        return Err(refused("URLs with credentials are refused"));
    }
    let loopback = is_loopback_host(&host);
    match url.scheme() {
        "https" if loopback || url.port().is_none() => {}
        "http" if loopback => {}
        _ => {
            return Err(refused(format!(
                "only https on the default port (http only for loopback hosts): {url}"
            )));
        }
    }
    if !host_allowed(allowed_hosts, &host_text(&host)) {
        return Err(refused(format!(
            "{} is not among the plug-in's network permissions",
            host_text(&host)
        )));
    }
    Ok(())
}

impl PluginHttp {
    /// Uses another resolver (tests: scripted DNS answers).
    #[cfg(test)]
    pub(crate) fn with_resolver(resolver: Arc<dyn Resolver>) -> Self {
        Self {
            tls: Arc::new(OnceLock::new()),
            resolver,
        }
    }

    fn tls(&self) -> Result<Arc<rustls::ClientConfig>, ApiError> {
        self.tls
            .get_or_init(|| match nail_auth::tls_config() {
                Ok(tls) => Some(Arc::new(tls)),
                Err(err) => {
                    tracing::warn!(%err, "no TLS configuration for plug-in requests");
                    None
                }
            })
            .clone()
            .ok_or_else(|| network("no TLS configuration"))
    }

    /// Executes `request` (see the module docs).
    pub(crate) async fn fetch(&self, request: HttpRequest) -> Result<HttpResponse, ApiError> {
        let timeout = request.timeout;
        match tokio::time::timeout(timeout, self.fetch_inner(request)).await {
            Ok(result) => result,
            Err(_) => Err(network("the request timed out")),
        }
    }

    async fn fetch_inner(&self, request: HttpRequest) -> Result<HttpResponse, ApiError> {
        let mut url = Url::parse(&request.url).map_err(|e| refused(format!("invalid URL: {e}")))?;
        url.set_fragment(None);
        let mut method = request.method.clone();
        let mut body = request.body.clone();
        let mut redirects = 0_u8;
        loop {
            check_hop(&url, &request.allowed_hosts)?;
            let response = self.hop(&url, &method, &request, body.clone()).await?;
            let status = response.status();
            if status.is_redirection()
                && let Some(location) = response.headers().get(reqwest::header::LOCATION)
            {
                if redirects >= MAX_REDIRECTS {
                    return Err(network("too many redirects"));
                }
                redirects += 1;
                let location = location.to_str().map_err(|_| network("invalid redirect"))?;
                let mut next = url
                    .join(location)
                    .map_err(|_| network("invalid redirect"))?;
                next.set_fragment(None);
                let code = status.as_u16();
                if code == 303 || (matches!(code, 301 | 302) && method == "POST") {
                    "GET".clone_into(&mut method);
                    body = None;
                }
                url = next;
                continue;
            }
            return read(response, request.max_response_bytes).await;
        }
    }

    /// One request, connected only to checked addresses.
    async fn hop(
        &self,
        url: &Url,
        method: &str,
        request: &HttpRequest,
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::Response, ApiError> {
        let port = url
            .port_or_known_default()
            .ok_or_else(|| refused(format!("{url} has no port")))?;
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .pool_max_idle_per_host(0)
            .user_agent(USER_AGENT)
            .timeout(request.timeout)
            .dns_resolver(Arc::new(RefuseResolver));
        // Also for `http:` (reqwest needs a crypto provider to build).
        builder = builder.tls_backend_preconfigured((*self.tls()?).clone());
        let allowed: Vec<IpAddr> = match url.host() {
            Some(Host::Domain(name)) => {
                let addrs = self
                    .resolver
                    .lookup(name)
                    .await
                    .map_err(|e| network(format!("cannot resolve {name}: {e}")))?;
                if addrs.is_empty() {
                    return Err(network(format!("cannot resolve {name}")));
                }
                if let Some(bad) = addrs.iter().find(|ip| !is_public_ip(**ip)) {
                    return Err(refused(format!(
                        "{name} resolves to the private or local address {bad}"
                    )));
                }
                let pinned: Vec<SocketAddr> =
                    addrs.iter().map(|ip| SocketAddr::new(*ip, port)).collect();
                builder = builder.resolve_to_addrs(name, &pinned);
                addrs
            }
            // A literal the plug-in's permission names (checked above).
            Some(Host::Ipv4(ip)) => vec![IpAddr::V4(ip)],
            Some(Host::Ipv6(ip)) => vec![IpAddr::V6(ip)],
            None => return Err(refused(format!("{url} has no host"))),
        };
        let client = builder.build().map_err(|e| network(e.to_string()))?;
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| refused(format!("method {method} is refused")))?;
        let mut outgoing = client.request(method, url.clone());
        for (name, value) in &request.headers {
            let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) else {
                return Err(refused(format!("invalid header {name}")));
            };
            outgoing = outgoing.header(name, value);
        }
        if let Some(body) = body {
            outgoing = outgoing.body(body);
        }
        let response = outgoing.send().await.map_err(|e| {
            if e.is_timeout() {
                network("the request timed out")
            } else {
                network(e.to_string())
            }
        })?;
        if let Some(peer) = response.remote_addr() {
            let ip = canonical(peer.ip());
            if !allowed.iter().any(|a| canonical(*a) == ip) {
                return Err(refused(format!("connected to an unchecked address {ip}")));
            }
        }
        Ok(response)
    }
}

async fn read(mut response: reqwest::Response, max: u64) -> Result<HttpResponse, ApiError> {
    let too_large = || {
        ApiError::new(
            ErrorCode::TooLarge,
            format!("the response is larger than {max} bytes"),
        )
    };
    if response.content_length().is_some_and(|len| len > max) {
        return Err(too_large());
    }
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_ascii_lowercase(), v.to_owned()))
        })
        .collect();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| network(e.to_string()))? {
        if body.len() as u64 + chunk.len() as u64 > max {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

/// IPv4-mapped IPv6 peers compare as IPv4.
pub(crate) fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// reqwest's resolver for pinned clients: every name must come from the
/// override map.
pub(crate) struct RefuseResolver;

impl reqwest::dns::Resolve for RefuseResolver {
    fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async {
            Err::<reqwest::dns::Addrs, _>(
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "unpinned lookup refused",
                )
                .into(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nail_proxy::fetch::Lookup;
    use nail_proxy::testing::{Reply, Upstream};

    use super::*;

    struct Fixed(Vec<IpAddr>);

    impl Resolver for Fixed {
        fn lookup<'a>(&'a self, _host: &'a str) -> Lookup<'a> {
            let addrs = self.0.clone();
            Box::pin(async move { Ok(addrs) })
        }
    }

    fn request(url: &str, hosts: &[&str]) -> HttpRequest {
        HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: vec![("x-test".into(), "1".into())],
            body: None,
            allowed_hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
            max_response_bytes: 1024,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn hop_rules() {
        let hosts = vec!["api.example.com".to_owned(), "127.0.0.1".to_owned()];
        let ok = |u: &str| check_hop(&Url::parse(u).unwrap(), &hosts);
        assert!(ok("https://api.example.com/x").is_ok());
        assert!(ok("http://127.0.0.1:8080/x").is_ok());
        assert!(ok("https://127.0.0.1:8443/x").is_ok());
        for bad in [
            "http://api.example.com/x",
            "https://api.example.com:8443/x",
            "https://user:pw@api.example.com/x",
            "https://other.example.com/x",
            "ftp://api.example.com/x",
            "http://localhost:8080/x",
        ] {
            let err = ok(bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::Forbidden, "{bad}");
        }
    }

    #[tokio::test]
    async fn fetches_a_named_literal_with_method_headers_and_body() {
        let upstream = Upstream::start().await.unwrap();
        upstream.route("/echo", Reply::ok("application/json", "{\"ok\":true}"));
        let http = PluginHttp::default();
        let url = format!("http://{}/echo", upstream.addr());
        let mut req = request(&url, &["127.0.0.1"]);
        req.method = "POST".into();
        req.body = Some(b"payload".to_vec());
        let response = http.fetch(req).await.unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{\"ok\":true}");
        assert!(
            response
                .headers
                .iter()
                .any(|(n, v)| n == "content-type" && v == "application/json")
        );
        let recorded = upstream.requests();
        let last = recorded.last().unwrap();
        assert_eq!(last.header("x-test"), Some("1"));
        assert_eq!(last.header("user-agent"), Some(USER_AGENT));
    }

    #[tokio::test]
    async fn names_never_reach_private_addresses() {
        let upstream = Upstream::start().await.unwrap();
        upstream.route("/x", Reply::ok("text/plain", "secret"));
        let http = PluginHttp::with_resolver(Arc::new(Fixed(vec![upstream.addr().ip()])));
        // `localhost` is a loopback host (http allowed) but resolves to a
        // loopback address: refused unless the literal is granted.
        let url = format!("http://localhost:{}/x", upstream.addr().port());
        let err = http.fetch(request(&url, &["localhost"])).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{err:?}");
    }

    #[tokio::test]
    async fn redirects_are_rechecked() {
        let upstream = Upstream::start().await.unwrap();
        upstream.route(
            "/away",
            Reply::redirect(302, "https://evil.example.net/steal"),
        );
        upstream.route("/next", Reply::redirect(303, "/final"));
        upstream.route("/final", Reply::ok("text/plain", "done"));
        let http = PluginHttp::default();
        let base = format!("http://{}", upstream.addr());
        let err = http
            .fetch(request(&format!("{base}/away"), &["127.0.0.1"]))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        let mut post = request(&format!("{base}/next"), &["127.0.0.1"]);
        post.method = "POST".into();
        post.body = Some(b"x".to_vec());
        let response = http.fetch(post).await.unwrap();
        assert_eq!(response.body, b"done");
        assert!(
            upstream
                .requests()
                .last()
                .unwrap()
                .request_line
                .starts_with("GET ")
        );
    }

    #[tokio::test]
    async fn large_and_slow_responses_fail() {
        let upstream = Upstream::start().await.unwrap();
        upstream.route("/big", Reply::ok("text/plain", vec![b'x'; 4096]));
        upstream.route(
            "/slow",
            Reply::ok("text/plain", "late").delayed(Duration::from_secs(3)),
        );
        let http = PluginHttp::default();
        let base = format!("http://{}", upstream.addr());
        let err = http
            .fetch(request(&format!("{base}/big"), &["127.0.0.1"]))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::TooLarge);
        let mut slow = request(&format!("{base}/slow"), &["127.0.0.1"]);
        slow.timeout = Duration::from_millis(300);
        let err = http.fetch(slow).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Network);
    }
}
