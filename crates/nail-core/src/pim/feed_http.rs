//! Fetching ICS subscriptions without SSRF (ADR 0064), with the rules of
//! `nail-proxy` and plug-in requests ([`crate::plugin_http`]):
//!
//! 1. per hop (the first URL and every redirect): `https:` (any port), no
//!    credentials in the URL; plain `http:` only to loopback hosts and only
//!    when the service allows it ([`crate::ServiceOptions::loopback_feeds`]:
//!    tests and seeded demos), so a `webcal://localhost:…` link from a web
//!    page cannot make Nail poll local services;
//! 2. a DNS name is resolved once and **every** answer must be public
//!    ([`nail_proxy::is_public_ip`]); IP literals must be public too
//!    (loopback literals only under the test exception); the connection is
//!    pinned to the checked addresses and the peer is checked again;
//! 3. redirects are followed by hand (at most [`MAX_REDIRECTS`]), each hop
//!    checked; the feed's login (`Authorization`) goes only to the origin
//!    (scheme, host, port) of the configured URL;
//! 4. the body is limited to `max_bytes`: a larger `Content-Length` is
//!    refused before reading, and the stream is cut at the limit;
//! 5. no cookies, no `Referer`, no system proxy; the whole fetch is bounded
//!    by [`FEED_TIMEOUT`].

use std::net::IpAddr;
use std::time::Duration;

use nail_api::{ApiError, ErrorCode};
use nail_proxy::{Resolver, is_public_ip};
use url::Url;

use crate::pinned_http::{
    check_peer, hop_host, is_loopback_host, network, redirect_target, refused, resolve_public,
};

/// Redirects followed per fetch.
pub(crate) const MAX_REDIRECTS: u8 = 5;

/// The longest a fetch may take, redirects included.
pub(crate) const FEED_TIMEOUT: Duration = Duration::from_mins(1);

/// One feed fetch.
pub(crate) struct FeedRequest {
    /// The configured URL (already mapped from `webcal:`).
    pub url: Url,
    /// `If-None-Match`.
    pub etag: Option<String>,
    /// `If-Modified-Since`.
    pub last_modified: Option<String>,
    /// Basic login (sent only to the configured URL's origin).
    pub basic: Option<(String, Option<String>)>,
    /// Plain `http:` to loopback hosts (tests, seeded demos).
    pub allow_loopback: bool,
    /// Largest body accepted.
    pub max_bytes: usize,
}

/// What a fetch brought.
#[derive(Debug)]
pub(crate) enum FeedResponse {
    /// `304`: unchanged.
    NotModified,
    /// The feed.
    Fetched {
        /// Its bytes.
        bytes: Vec<u8>,
        /// Its `ETag`.
        etag: Option<String>,
        /// Its `Last-Modified`.
        last_modified: Option<String>,
    },
}

/// The URL rules of one hop.
pub(crate) fn check_hop(url: &Url, allow_loopback: bool) -> Result<(), ApiError> {
    let host = hop_host(url, "feed URLs")?;
    let loopback = is_loopback_host(&host);
    if loopback && !allow_loopback {
        return Err(refused(format!(
            "feeds on this computer are refused: {url}"
        )));
    }
    match url.scheme() {
        "https" => Ok(()),
        "http" if loopback => Ok(()),
        _ => Err(refused(format!("only https feeds are fetched: {url}"))),
    }
}

/// Whether two URLs share scheme, host and port.
fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str().map(str::to_ascii_lowercase) == b.host_str().map(str::to_ascii_lowercase)
        && a.port_or_known_default() == b.port_or_known_default()
}

/// Fetches a feed (see the module docs).
pub(crate) async fn fetch(
    request: &FeedRequest,
    resolver: &dyn Resolver,
    tls: &rustls::ClientConfig,
) -> Result<FeedResponse, ApiError> {
    match tokio::time::timeout(FEED_TIMEOUT, fetch_inner(request, resolver, tls)).await {
        Ok(result) => result,
        Err(_) => Err(network("the feed did not answer in time")),
    }
}

async fn fetch_inner(
    request: &FeedRequest,
    resolver: &dyn Resolver,
    tls: &rustls::ClientConfig,
) -> Result<FeedResponse, ApiError> {
    let mut url = request.url.clone();
    url.set_fragment(None);
    let mut redirects = 0_u8;
    loop {
        check_hop(&url, request.allow_loopback)?;
        let login = same_origin(&url, &request.url)
            .then_some(request.basic.as_ref())
            .flatten();
        let response = hop(&url, request, login, resolver, tls).await?;
        let status = response.status();
        if status.is_redirection()
            && status != reqwest::StatusCode::NOT_MODIFIED
            && let Some(location) = response.headers().get(reqwest::header::LOCATION)
        {
            if redirects >= MAX_REDIRECTS {
                return Err(network("the feed redirects too often"));
            }
            redirects += 1;
            url = redirect_target(&url, location, "the feed sent an invalid redirect")?;
            continue;
        }
        return read(response, request.max_bytes).await;
    }
}

/// One GET, connected only to checked addresses.
async fn hop(
    url: &Url,
    request: &FeedRequest,
    login: Option<&(String, Option<String>)>,
    resolver: &dyn Resolver,
    tls: &rustls::ClientConfig,
) -> Result<reqwest::Response, ApiError> {
    let port = url
        .port_or_known_default()
        .ok_or_else(|| refused(format!("{url} has no port")))?;
    let builder = nail_proxy::pinned_builder(
        tls,
        concat!("Nail/", env!("CARGO_PKG_VERSION")),
        FEED_TIMEOUT,
    );
    let (builder, allowed) = nail_proxy::pin_host(
        builder,
        url,
        port,
        async |name: &str| {
            if name.eq_ignore_ascii_case("localhost") {
                // Allowed only under the test exception (checked above).
                Ok(vec![IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)])
            } else {
                resolve_public(resolver, name).await
            }
        },
        || refused(format!("{url} has no host")),
    )
    .await?;
    for ip in &allowed {
        let local_ok = request.allow_loopback && nail_proxy::canonical(*ip).is_loopback();
        if !is_public_ip(*ip) && !local_ok {
            return Err(refused(format!("{ip} is a private or local address")));
        }
    }
    let client = builder.build().map_err(|e| network(e.to_string()))?;
    let mut outgoing = client.get(url.clone()).header(
        reqwest::header::ACCEPT,
        "text/calendar, text/plain;q=0.5, */*;q=0.1",
    );
    if let Some(etag) = &request.etag {
        outgoing = outgoing.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    if let Some(modified) = &request.last_modified {
        outgoing = outgoing.header(reqwest::header::IF_MODIFIED_SINCE, modified);
    }
    if let Some((username, password)) = login {
        outgoing = outgoing.basic_auth(username, password.as_ref());
    }
    let response = outgoing.send().await.map_err(|e| {
        if e.is_timeout() {
            network("the feed did not answer in time")
        } else {
            network(format!("cannot fetch the feed: {e}"))
        }
    })?;
    check_peer(&response, &allowed)?;
    Ok(response)
}

async fn read(mut response: reqwest::Response, max: usize) -> Result<FeedResponse, ApiError> {
    let status = response.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(FeedResponse::NotModified);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ApiError::new(
            ErrorCode::AuthFailed,
            format!("the feed answered {status}"),
        ));
    }
    if !status.is_success() {
        let code = if status.is_server_error() {
            ErrorCode::Unavailable
        } else {
            ErrorCode::NotFound
        };
        return Err(ApiError::new(code, format!("the feed answered {status}")));
    }
    let header = |name: reqwest::header::HeaderName| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let etag = header(reqwest::header::ETAG);
    let last_modified = header(reqwest::header::LAST_MODIFIED);
    let bytes = nail_proxy::read_limited(&mut response, u64::try_from(max).unwrap_or(u64::MAX))
        .await
        .map_err(|e| match e {
            nail_proxy::BodyError::TooLarge => {
                ApiError::new(ErrorCode::TooLarge, "the feed is too large")
            }
            nail_proxy::BodyError::Network(e) => network(format!("cannot read the feed: {e}")),
        })?;
    Ok(FeedResponse::Fetched {
        bytes,
        etag,
        last_modified,
    })
}

#[cfg(test)]
mod tests {
    use nail_proxy::testing::{Reply, ScriptedResolver, Upstream};

    use super::*;

    fn tls() -> rustls::ClientConfig {
        nail_auth::tls_config().unwrap()
    }

    fn request(url: &str, allow_loopback: bool) -> FeedRequest {
        FeedRequest {
            url: Url::parse(url).unwrap(),
            etag: None,
            last_modified: None,
            basic: None,
            allow_loopback,
            max_bytes: 1024,
        }
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n";

    #[test]
    fn hop_rules() {
        let ok = |u: &str, l: bool| check_hop(&Url::parse(u).unwrap(), l);
        assert!(ok("https://feeds.example.com/a.ics", false).is_ok());
        assert!(ok("https://feeds.example.com:8443/a.ics", false).is_ok());
        assert!(ok("http://127.0.0.1:8080/a.ics", true).is_ok());
        assert!(ok("http://localhost:8080/a.ics", true).is_ok());
        for (bad, loopback) in [
            ("http://feeds.example.com/a.ics", false),
            ("http://feeds.example.com/a.ics", true),
            ("http://127.0.0.1:8080/a.ics", false),
            ("http://localhost:8080/a.ics", false),
            ("https://localhost/a.ics", false),
            ("https://[::1]/a.ics", false),
            ("https://user:pw@feeds.example.com/a.ics", false),
            ("ftp://feeds.example.com/a.ics", false),
        ] {
            assert_eq!(
                ok(bad, loopback).unwrap_err().code,
                ErrorCode::Forbidden,
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn loopback_feeds_need_the_test_exception() {
        let upstream = Upstream::start().await.unwrap();
        upstream.route("/a.ics", Reply::ok("text/calendar", ICS));
        let resolver = ScriptedResolver::new();
        let url = format!("http://{}/a.ics", upstream.addr());
        let err = fetch(&request(&url, false), resolver.as_ref(), &tls())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert_eq!(upstream.connections(), 0);
        let FeedResponse::Fetched { bytes, .. } =
            fetch(&request(&url, true), resolver.as_ref(), &tls())
                .await
                .unwrap()
        else {
            panic!("fetched")
        };
        assert_eq!(bytes, ICS.as_bytes());
    }

    #[tokio::test]
    async fn names_and_literals_never_reach_private_addresses() {
        let resolver = ScriptedResolver::new();
        resolver.set("intranet.example.com", &["10.1.2.3"]);
        resolver.set("mixed.example.com", &["93.184.216.34", "192.168.1.1"]);
        resolver.set("metadata.example.com", &["169.254.169.254"]);
        for url in [
            "https://intranet.example.com/a.ics",
            "https://mixed.example.com/a.ics",
            "https://metadata.example.com/a.ics",
            "https://10.0.0.1/a.ics",
            "https://192.168.0.10:8443/a.ics",
            "https://[fd00::1]/a.ics",
            "https://169.254.169.254/latest",
        ] {
            let err = fetch(&request(url, true), resolver.as_ref(), &tls())
                .await
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::Forbidden, "{url}: {err:?}");
        }
    }

    #[tokio::test]
    async fn redirects_are_followed_rechecked_and_drop_the_login() {
        let first = Upstream::start().await.unwrap();
        let second = Upstream::start().await.unwrap();
        first.route(
            "/moved",
            Reply::redirect(301, &format!("http://{}/feed.ics", second.addr())),
        );
        first.route("/here", Reply::redirect(302, "/feed.ics"));
        first.route("/feed.ics", Reply::ok("text/calendar", ICS));
        second.route("/feed.ics", Reply::ok("text/calendar", ICS));
        first.route(
            "/evil",
            Reply::redirect(302, "https://intranet.example.com/admin"),
        );
        first.route("/loop", Reply::redirect(302, "/loop"));
        let resolver = ScriptedResolver::new();
        resolver.set("intranet.example.com", &["10.1.2.3"]);
        let base = format!("http://{}", first.addr());
        let with_login = |path: &str| {
            let mut r = request(&format!("{base}{path}"), true);
            r.basic = Some(("ada".into(), Some("secret".into())));
            r
        };
        // Same origin: followed, the login goes along.
        assert!(matches!(
            fetch(&with_login("/here"), resolver.as_ref(), &tls())
                .await
                .unwrap(),
            FeedResponse::Fetched { .. }
        ));
        let to_feed = first.requests();
        assert!(to_feed.iter().all(|r| r.header("authorization").is_some()));
        // Another origin: followed without the login.
        assert!(matches!(
            fetch(&with_login("/moved"), resolver.as_ref(), &tls())
                .await
                .unwrap(),
            FeedResponse::Fetched { .. }
        ));
        let elsewhere = second.requests();
        assert_eq!(elsewhere.len(), 1);
        assert_eq!(elsewhere[0].header("authorization"), None);
        // A redirect into the intranet is refused; loops end.
        let err = fetch(&with_login("/evil"), resolver.as_ref(), &tls())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        let err = fetch(&with_login("/loop"), resolver.as_ref(), &tls())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Network);
    }

    #[tokio::test]
    async fn large_bodies_are_refused_before_and_while_reading() {
        let upstream = Upstream::start().await.unwrap();
        let big = "X".repeat(4096);
        upstream.route("/declared", Reply::ok("text/calendar", big.clone()));
        upstream.route(
            "/streamed",
            Reply::ok("text/calendar", big).without_length(),
        );
        upstream.route("/unchanged", Reply::status(304));
        let resolver = ScriptedResolver::new();
        for path in ["/declared", "/streamed"] {
            let url = format!("http://{}{path}", upstream.addr());
            let err = fetch(&request(&url, true), resolver.as_ref(), &tls())
                .await
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::TooLarge, "{path}");
        }
        let mut conditional = request(&format!("http://{}/unchanged", upstream.addr()), true);
        conditional.etag = Some("\"v1\"".into());
        assert!(matches!(
            fetch(&conditional, resolver.as_ref(), &tls())
                .await
                .unwrap(),
            FeedResponse::NotModified
        ));
        let last = upstream.requests();
        assert_eq!(last.last().unwrap().header("if-none-match"), Some("\"v1\""));
    }
}
