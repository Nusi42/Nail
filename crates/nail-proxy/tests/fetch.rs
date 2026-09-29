//! The fetcher and the proxy against loopback mock upstreams only: SSRF
//! attempts (literal addresses in every spelling, names resolving to
//! private ranges, IPv6 / IPv4-mapped forms, redirects into private
//! ranges, DNS rebinding), header hygiene and the size / type / time
//! limits.
//!
//! The mock upstream listens on loopback, which the fetcher refuses; the
//! tests open exactly its port with `allow_loopback_port_for_tests`
//! (feature `test-support`) where a fetch is meant to succeed.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use nail_proxy::testing::{Reply, ScriptedResolver, Upstream};
use nail_proxy::{
    Check, FetchError, FetchLimits, Fetcher, Proxy, ProxyError, ProxyKey, Signer, TargetCheck,
    TokenError, UrlRefusal,
};
use pretty_assertions::assert_eq;
use url::Url;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n-not-really-a-png";

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A client TLS configuration trusting nothing (the HTTP tests never use
/// it) or the given roots.
fn client_tls(roots: rustls::RootCertStore) -> Arc<rustls::ClientConfig> {
    Arc::new(
        rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn fetcher(resolver: &Arc<ScriptedResolver>) -> Fetcher {
    Fetcher::new(client_tls(rustls::RootCertStore::empty()))
        .with_resolver(Arc::clone(resolver) as Arc<dyn nail_proxy::Resolver>)
}

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[tokio::test]
async fn fetches_an_allowed_image_without_leaking_anything() {
    let up = Upstream::start().await.unwrap();
    up.route(
        "/logo.png?v=1",
        Reply::ok("image/png", PNG).header("Set-Cookie", "tracker=1"),
    );
    let dns = ScriptedResolver::new();
    dns.set("img.example.org", &["127.0.0.1"]);
    let f = fetcher(&dns).allow_loopback_port_for_tests(up.port());

    let target = url(&format!(
        "http://img.example.org:{}/logo.png?v=1#frag",
        up.port()
    ));
    let fetched = f.fetch(&target).await.unwrap();
    assert_eq!(fetched.content_type, "image/png");
    assert_eq!(fetched.body, PNG);
    assert_eq!(dns.lookups(), vec!["img.example.org"]);

    let requests = up.requests();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(req.request_line, "GET /logo.png?v=1 HTTP/1.1");
    assert_eq!(
        req.header("host"),
        Some(format!("img.example.org:{}", up.port()).as_str())
    );
    assert_eq!(
        req.header("user-agent"),
        Some(nail_proxy::fetch::USER_AGENT)
    );
    assert_eq!(req.header("accept-encoding"), Some("identity"));
    for leaked in [
        "cookie",
        "authorization",
        "referer",
        "origin",
        "proxy-authorization",
        "x-forwarded-for",
        "forwarded",
    ] {
        assert_eq!(req.header(leaked), None, "{leaked} must not be sent");
    }

    // A second fetch sends no cookie from the first answer's Set-Cookie.
    f.fetch(&target).await.unwrap();
    assert_eq!(up.requests()[1].header("cookie"), None);
}

#[tokio::test]
async fn literal_private_addresses_never_connect() {
    let up = Upstream::start().await.unwrap();
    up.route("/a.png", Reply::ok("image/png", PNG));
    let dns = ScriptedResolver::new();
    // Not exempted at all, then exempted for another port only.
    for f in [
        fetcher(&dns),
        fetcher(&dns).allow_loopback_port_for_tests(up.port().wrapping_add(1)),
    ] {
        let p = up.port();
        for target in [
            format!("http://127.0.0.1:{p}/a.png"),
            format!("http://2130706433:{p}/a.png"),
            format!("http://0x7f.0.0.1:{p}/a.png"),
            format!("http://0177.0.0.1:{p}/a.png"),
            format!("http://127.1:{p}/a.png"),
            format!("http://[::1]:{p}/a.png"),
            format!("http://[::ffff:127.0.0.1]:{p}/a.png"),
            format!("http://[::ffff:7f00:1]:{p}/a.png"),
            format!("http://[64:ff9b::7f00:1]:{p}/a.png"),
            "http://169.254.169.254/latest/meta-data/".to_owned(),
            "http://[fd00:ec2::254]/".to_owned(),
            "http://10.0.0.1/".to_owned(),
            "http://192.168.1.1/".to_owned(),
            "http://0.0.0.0/".to_owned(),
            "http://[::]/".to_owned(),
            "http://224.0.0.1/".to_owned(),
        ] {
            let err = f.fetch(&url(&target)).await.unwrap_err();
            assert!(
                matches!(err, FetchError::Blocked(_)),
                "{target}: expected Blocked, got {err:?}"
            );
        }
    }
    assert_eq!(up.connections(), 0);
    assert!(dns.lookups().is_empty());
}

#[tokio::test]
async fn names_resolving_to_private_addresses_are_refused_before_connecting() {
    let up = Upstream::start().await.unwrap();
    up.route("/a.png", Reply::ok("image/png", PNG));
    let dns = ScriptedResolver::new();
    let f = fetcher(&dns);
    let cases: &[(&str, &[&str])] = &[
        ("loop.example.org", &["127.0.0.1"]),
        ("v6loop.example.org", &["::1"]),
        ("mapped.example.org", &["::ffff:127.0.0.1"]),
        ("metadata.example.org", &["169.254.169.254"]),
        ("lan.example.org", &["192.168.0.10"]),
        ("ula.example.org", &["fd12:3456::1"]),
        ("linklocal6.example.org", &["fe80::1"]),
        ("cgnat.example.org", &["100.64.1.1"]),
        ("multicast.example.org", &["239.1.1.1"]),
        // One bad answer among good ones refuses the whole name.
        ("mixed.example.org", &["93.184.216.34", "127.0.0.1"]),
        ("mixed6.example.org", &["2606:2800:220:1::1", "::1"]),
    ];
    for (name, addrs) in cases {
        dns.set(name, addrs);
        let target = url(&format!("http://{name}:{}/a.png", up.port()));
        let err = f.fetch(&target).await.unwrap_err();
        assert!(
            matches!(err, FetchError::Blocked(_)),
            "{name}: expected Blocked, got {err:?}"
        );
    }
    assert_eq!(up.connections(), 0);

    // Local-only names are refused before resolving.
    for name in ["localhost", "intranet", "nas.local", "x.localhost"] {
        let err = f
            .fetch(&url(&format!("http://{name}:{}/a.png", up.port())))
            .await
            .unwrap_err();
        assert_eq!(err, FetchError::Url(UrlRefusal::LocalName), "{name}");
    }
    // An unknown name is a resolve error.
    assert_eq!(
        f.fetch(&url("http://nowhere.example.org/a.png"))
            .await
            .unwrap_err(),
        FetchError::Resolve
    );
}

/// DNS rebinding: the attacker's name first answers an allowed address,
/// then a private one. The fetcher resolves once per hop and connects only
/// to what it checked, so the second answer is never used for that fetch;
/// a later fetch sees the private answer and is refused.
#[tokio::test]
async fn dns_rebinding_cannot_redirect_the_connection() {
    let up = Upstream::start().await.unwrap();
    up.route("/a.png", Reply::ok("image/png", PNG));
    let dns = ScriptedResolver::new();
    dns.script(
        "rebind.example.org",
        vec![vec![ip("127.0.0.1")], vec![ip("10.0.0.5")]],
    );
    let f = fetcher(&dns).allow_loopback_port_for_tests(up.port());
    let target = url(&format!("http://rebind.example.org:{}/a.png", up.port()));

    let fetched = f.fetch(&target).await.unwrap();
    assert_eq!(fetched.body, PNG);
    // Exactly one lookup for the fetch: the HTTP client never re-resolved.
    assert_eq!(dns.lookups().len(), 1);
    assert_eq!(up.connections(), 1);

    let err = f.fetch(&target).await.unwrap_err();
    assert_eq!(err, FetchError::Blocked(ip("10.0.0.5")));
    assert_eq!(dns.lookups().len(), 2);
    assert_eq!(up.connections(), 1);
}

#[tokio::test]
async fn every_redirect_is_checked_like_the_first_url() {
    let up = Upstream::start().await.unwrap();
    let other = Upstream::start().await.unwrap();
    other.route("/secret", Reply::ok("image/png", b"internal".to_vec()));
    up.route("/a.png", Reply::ok("image/png", PNG));
    let p = up.port();
    let o = other.port();
    let dns = ScriptedResolver::new();
    dns.set("img.example.org", &["127.0.0.1"]);
    dns.set("evil.example.org", &["127.0.0.1"]);
    dns.set("private.example.org", &["192.168.1.1"]);
    let f = fetcher(&dns).allow_loopback_port_for_tests(p);

    let refused: Vec<(String, FetchError)> = vec![
        // Another loopback port (the exemption covers one port only).
        (
            format!("http://127.0.0.1:{o}/secret"),
            FetchError::Blocked(ip("127.0.0.1")),
        ),
        (
            format!("http://[::ffff:127.0.0.1]:{o}/secret"),
            FetchError::Blocked(ip("::ffff:127.0.0.1")),
        ),
        (
            "http://169.254.169.254/latest/meta-data/".to_owned(),
            FetchError::Blocked(ip("169.254.169.254")),
        ),
        (
            "http://private.example.org/x.png".to_owned(),
            FetchError::Blocked(ip("192.168.1.1")),
        ),
        (
            format!("http://localhost:{p}/a.png"),
            FetchError::Url(UrlRefusal::LocalName),
        ),
        (
            "ftp://img.example.org/a.png".to_owned(),
            FetchError::Url(UrlRefusal::Scheme),
        ),
        (
            "file:///C:/Windows/win.ini".to_owned(),
            FetchError::Url(UrlRefusal::Scheme),
        ),
        (
            format!("http://user:pw@img.example.org:{p}/a.png"),
            FetchError::Url(UrlRefusal::Credentials),
        ),
        (
            "http://img.example.org:25/".to_owned(),
            FetchError::Url(UrlRefusal::Port(25)),
        ),
    ];
    for (i, (location, expected)) in refused.iter().enumerate() {
        let path = format!("/r{i}");
        up.route(&path, Reply::redirect(302, location));
        let err = f
            .fetch(&url(&format!("http://img.example.org:{p}{path}")))
            .await
            .unwrap_err();
        assert_eq!(&err, expected, "redirect to {location}");
    }
    assert_eq!(other.connections(), 0);

    // Allowed redirects are followed (relative, absolute via another name,
    // every redirect status); the new name is resolved and checked again.
    up.route("/rel", Reply::redirect(301, "/a.png"));
    up.route(
        "/abs",
        Reply::redirect(307, &format!("http://evil.example.org:{p}/a.png")),
    );
    up.route("/see-other", Reply::redirect(303, "a.png"));
    up.route("/perm", Reply::redirect(308, "/rel"));
    for path in ["/rel", "/abs", "/see-other", "/perm"] {
        let fetched = f
            .fetch(&url(&format!("http://img.example.org:{p}{path}")))
            .await
            .unwrap();
        assert_eq!(fetched.body, PNG, "{path}");
        assert_eq!(fetched.final_url.path(), "/a.png");
    }
    assert!(dns.lookups().contains(&"evil.example.org".to_owned()));

    // Redirect loops and chains end at the limit; a redirect without a
    // Location is an error.
    up.route("/loop", Reply::redirect(302, "/loop"));
    assert_eq!(
        f.fetch(&url(&format!("http://img.example.org:{p}/loop")))
            .await
            .unwrap_err(),
        FetchError::Redirects
    );
    up.route("/nowhere", Reply::status(302));
    assert_eq!(
        f.fetch(&url(&format!("http://img.example.org:{p}/nowhere")))
            .await
            .unwrap_err(),
        FetchError::Redirects
    );
}

#[tokio::test]
async fn size_type_status_and_time_limits() {
    let up = Upstream::start().await.unwrap();
    let dns = ScriptedResolver::new();
    dns.set("img.example.org", &["127.0.0.1"]);
    let limits = FetchLimits {
        max_bytes: 1024,
        timeout: Duration::from_millis(1500),
        ..FetchLimits::default()
    };
    let f = fetcher(&dns)
        .allow_loopback_port_for_tests(up.port())
        .with_limits(limits);
    let get = |path: &str| url(&format!("http://img.example.org:{}{path}", up.port()));

    up.route("/big", Reply::ok("image/png", vec![0_u8; 2048]));
    assert_eq!(
        f.fetch(&get("/big")).await.unwrap_err(),
        FetchError::TooLarge
    );
    up.route(
        "/big-unannounced",
        Reply::ok("image/png", vec![0_u8; 4096]).without_length(),
    );
    assert_eq!(
        f.fetch(&get("/big-unannounced")).await.unwrap_err(),
        FetchError::TooLarge
    );
    up.route("/exact", Reply::ok("image/gif", vec![1_u8; 1024]));
    assert_eq!(f.fetch(&get("/exact")).await.unwrap().body.len(), 1024);

    for (path, content_type) in [
        ("/page", "text/html; charset=utf-8"),
        ("/vector", "image/svg+xml"),
        ("/script", "application/javascript"),
        ("/json", "application/json"),
    ] {
        up.route(
            path,
            Reply::ok(content_type, b"<script>alert(1)</script>".to_vec()),
        );
        assert_eq!(
            f.fetch(&get(path)).await.unwrap_err(),
            FetchError::UnsupportedType(content_type.to_owned()),
            "{path}"
        );
    }
    up.route("/untyped", Reply::status(200));
    assert!(matches!(
        f.fetch(&get("/untyped")).await.unwrap_err(),
        FetchError::UnsupportedType(_)
    ));

    assert_eq!(
        f.fetch(&get("/missing")).await.unwrap_err(),
        FetchError::Status(404)
    );
    up.route(
        "/slow",
        Reply::ok("image/png", PNG).delayed(Duration::from_secs(5)),
    );
    let started = std::time::Instant::now();
    assert_eq!(
        f.fetch(&get("/slow")).await.unwrap_err(),
        FetchError::Timeout
    );
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[tokio::test]
async fn https_upstreams_are_verified_and_use_the_host_name() {
    let certified = rcgen::generate_simple_self_signed(vec!["img.example.org".to_owned()]).unwrap();
    let cert = certified.cert.der().clone();
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let server = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key)
        .unwrap();
    let up = Upstream::start_tls(Arc::new(server)).await.unwrap();
    up.route("/a.png", Reply::ok("image/png", PNG));
    let dns = ScriptedResolver::new();
    dns.set("img.example.org", &["127.0.0.1"]);
    dns.set("other.example.org", &["127.0.0.1"]);

    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).unwrap();
    let f = Fetcher::new(client_tls(roots))
        .with_resolver(Arc::clone(&dns) as Arc<dyn nail_proxy::Resolver>)
        .allow_loopback_port_for_tests(up.port());
    let fetched = f
        .fetch(&url(&format!(
            "https://img.example.org:{}/a.png",
            up.port()
        )))
        .await
        .unwrap();
    assert_eq!(fetched.body, PNG);

    // The certificate does not name other.example.org: refused.
    let err = f
        .fetch(&url(&format!(
            "https://other.example.org:{}/a.png",
            up.port()
        )))
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::Network(_)), "{err:?}");
}

struct DenyAll;

impl TargetCheck for DenyAll {
    fn allows<'a>(&'a self, _message_id: &'a str, _url: &'a Url) -> Check<'a> {
        Box::pin(async { false })
    }
}

struct OnlyMessage(&'static str);

impl TargetCheck for OnlyMessage {
    fn allows<'a>(&'a self, message_id: &'a str, _url: &'a Url) -> Check<'a> {
        let ok = message_id == self.0;
        Box::pin(async move { ok })
    }
}

#[tokio::test]
async fn the_proxy_serves_only_signed_urls() {
    let up = Upstream::start().await.unwrap();
    up.route("/a.png", Reply::ok("image/png", PNG));
    let dns = ScriptedResolver::new();
    dns.set("img.example.org", &["127.0.0.1"]);
    let signer = Signer::new(ProxyKey::generate());
    let proxy = Proxy::new(
        signer.clone(),
        fetcher(&dns).allow_loopback_port_for_tests(up.port()),
    );
    let target = format!("http://img.example.org:{}/a.png", up.port());

    let token = signer.sign_resource("m-1", &target).unwrap();
    let response = proxy.serve(&token).await.unwrap();
    assert_eq!(response.content_type, "image/png");
    assert_eq!(response.body, PNG);
    assert!(response.cache_control.starts_with("private, max-age="));
    // The `nail-proxy:` form works too.
    let proxied = signer.proxy_url("m-1", &target).unwrap();
    assert!(proxy.serve(&proxied).await.is_ok());

    // Not an open proxy: a URL without a token, a token from another key,
    // a render token.
    assert_eq!(
        proxy.serve(&target).await.unwrap_err(),
        ProxyError::Token(TokenError::Invalid)
    );
    let foreign = Signer::new(ProxyKey::generate())
        .sign_resource("m-1", &target)
        .unwrap();
    assert_eq!(
        proxy.serve(&foreign).await.unwrap_err(),
        ProxyError::Token(TokenError::Invalid)
    );
    let render = signer.sign_render("m-1").unwrap();
    assert_eq!(
        proxy.serve(&render).await.unwrap_err(),
        ProxyError::Token(TokenError::WrongPurpose)
    );
    let requests_before = up.requests().len();

    // The core's check can withdraw the permission.
    let denied = Proxy::new(
        signer.clone(),
        fetcher(&dns).allow_loopback_port_for_tests(up.port()),
    )
    .with_check(Arc::new(DenyAll));
    assert_eq!(denied.serve(&token).await.unwrap_err(), ProxyError::Denied);
    let scoped = Proxy::new(
        signer.clone(),
        fetcher(&dns).allow_loopback_port_for_tests(up.port()),
    )
    .with_check(Arc::new(OnlyMessage("m-2")));
    assert_eq!(scoped.serve(&token).await.unwrap_err(), ProxyError::Denied);
    let other_message = signer.sign_resource("m-2", &target).unwrap();
    assert!(scoped.serve(&other_message).await.is_ok());
    assert_eq!(up.requests().len(), requests_before + 1);

    // A signed URL that later resolves privately is still refused.
    dns.set("img.example.org", &["10.1.2.3"]);
    assert_eq!(
        proxy.serve(&token).await.unwrap_err().status(),
        403,
        "private after signing"
    );
}
