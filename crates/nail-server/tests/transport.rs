//! Transport and browser hardening over real sockets (plan M2b WP42): TLS
//! 1.3 / 1.2 with a persisted self-signed certificate, security headers,
//! the sandbox origin's isolation from the app origin, the proxy route
//! (only signed URLs, private ranges refused) and the listener rules.

#![cfg(test)]

mod common;

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;

use chrono::Utc;
use nail_core::Service;
use nail_proxy::testing::{Reply, ScriptedResolver, Upstream};
use nail_proxy::{Fetcher, Proxy, ProxyKey, Signer};
use nail_server::cert::{self, ServerIdentity};
use nail_server::sandbox::HtmlLookup;
use nail_server::{
    HeaderProfile, MailSource, SandboxOptions, Server, ServerError, ServerOptions, TransportOptions,
};
use pretty_assertions::assert_eq;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use common::{Fixture, Http, service};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

fn loopback() -> ServerOptions {
    ServerOptions {
        addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        ui_dir: None,
        test_control: None,
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn identity() -> ServerIdentity {
    let names = vec![
        "nail.test".to_owned(),
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
    ];
    cert::generate_self_signed(&names, Utc::now()).unwrap()
}

/// Connects to `addr` as `nail.test` over `version`, pinning `identity`.
async fn connect_tls(
    addr: SocketAddr,
    identity: &ServerIdentity,
    version: &'static rustls::SupportedProtocolVersion,
) -> std::io::Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let tcp = TcpStream::connect(addr).await?;
    pinned_client(identity, &[version])
        .connect(ServerName::try_from("nail.test").unwrap(), tcp)
        .await
}

/// A client that pins exactly `identity`'s certificate.
fn pinned_client(
    identity: &ServerIdentity,
    versions: &[&'static rustls::SupportedProtocolVersion],
) -> tokio_rustls::TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(identity.chain()[0].clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(versions)
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tokio_rustls::TlsConnector::from(Arc::new(config))
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> Http {
    let mut request = format!("{method} {path} HTTP/1.1\r\n");
    for (name, value) in headers {
        let _ = write!(request, "{name}: {value}\r\n");
    }
    request.push_str("Connection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
    let mut raw = Vec::new();
    drop(
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            stream.read_to_end(&mut raw),
        )
        .await
        .expect("response within 10 s"),
    );
    Http::parse(&raw)
}

/// `GET path` over plain TCP with `Host: 127.0.0.1:<port>` plus `extra`.
async fn get(addr: SocketAddr, path: &str, extra: &[(&str, &str)]) -> Http {
    let host = addr.to_string();
    let mut headers = vec![("Host", host.as_str())];
    headers.extend_from_slice(extra);
    let stream = TcpStream::connect(addr).await.unwrap();
    exchange(stream, "GET", path, &headers).await
}

// ---------------------------------------------------------------------------
// TLS

#[tokio::test]
async fn serves_https_over_tls13_and_tls12_with_a_pinned_certificate() {
    let f = service();
    let id = identity();
    let server = Server::bind_with(
        f.service.clone(),
        loopback(),
        TransportOptions {
            tls: Some(id.tls_config().unwrap()),
            host_names: vec!["nail.test".into()],
            headers: HeaderProfile::Strict,
            ..TransportOptions::default()
        },
    )
    .await
    .unwrap();
    let addr = server.local_addr();
    assert!(server.url().starts_with("https://127.0.0.1:"));

    // TLS 1.3.
    let tls = connect_tls(addr, &id, &rustls::version::TLS13)
        .await
        .unwrap();
    let (_, conn) = tls.get_ref();
    assert_eq!(
        conn.protocol_version(),
        Some(rustls::ProtocolVersion::TLSv1_3)
    );
    assert_eq!(conn.alpn_protocol(), None);
    let host = format!("nail.test:{}", addr.port());
    let response = exchange(tls, "GET", "/api/nope", &[("Host", &host)]).await;
    assert_eq!(response.status, 404);
    assert_eq!(
        response.header("strict-transport-security"),
        Some("max-age=31536000")
    );
    let csp = response.header("content-security-policy").unwrap();
    assert!(csp.contains("require-trusted-types-for 'script'"));
    assert!(csp.contains("upgrade-insecure-requests"));

    // TLS 1.2: only forward-secret AEAD suites are negotiated.
    let tls = connect_tls(addr, &id, &rustls::version::TLS12)
        .await
        .unwrap();
    let (_, conn) = tls.get_ref();
    assert_eq!(
        conn.protocol_version(),
        Some(rustls::ProtocolVersion::TLSv1_2)
    );
    let suite = format!("{:?}", conn.negotiated_cipher_suite().unwrap().suite());
    assert!(suite.starts_with("TLS_ECDHE_ECDSA_WITH_"), "{suite}");
    assert!(
        suite.contains("GCM") || suite.contains("CHACHA20_POLY1305"),
        "{suite}"
    );

    // The WebSocket upgrade works over TLS (and is Origin-checked).
    let tls = connect_tls(addr, &id, &rustls::version::TLS13)
        .await
        .unwrap();
    let origin = format!("https://nail.test:{}", addr.port());
    let mut stream = tls;
    let request = format!(
        "GET /api/ws HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = [0_u8; 256];
    let n = stream.read(&mut buf).await.unwrap();
    assert!(
        String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&buf[..n])
    );

    // A client that does not trust the certificate fails the handshake.
    let other = identity();
    assert!(
        connect_tls(addr, &other, &rustls::version::TLS13)
            .await
            .is_err()
    );

    // Plain HTTP to the TLS port gets no HTTP answer.
    let tcp = TcpStream::connect(addr).await.unwrap();
    let mut tcp = tcp;
    tcp.write_all(format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut raw = Vec::new();
    drop(tokio::time::timeout(std::time::Duration::from_secs(5), tcp.read_to_end(&mut raw)).await);
    assert!(!raw.starts_with(b"HTTP/1.1 200"));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_certificate_persists_across_restarts_with_its_fingerprint() {
    let f = service();
    let names = vec!["nail.test".to_owned(), "127.0.0.1".to_owned()];
    let first = cert::load_or_create_self_signed(&f.service, &names, Utc::now()).unwrap();
    let again = cert::load_or_create_self_signed(&f.service, &names, Utc::now()).unwrap();
    assert_eq!(first.fingerprint(), again.fingerprint());
    assert_eq!(first.fingerprint(), cert::fingerprint(&first.chain()[0]));
    // The store holds the record with a keychain reference, never the key.
    let record: serde_json::Value = f
        .service
        .store()
        .setting(nail_store::REMOTE_UI_CERTIFICATE_KEY)
        .unwrap()
        .unwrap();
    assert_eq!(record["fingerprint_sha256"], first.fingerprint());
    assert_eq!(record["key_ref"], cert::CERTIFICATE_KEY_REF);
    assert!(!record.to_string().contains("PRIVATE KEY"));
    // Regenerating changes the fingerprint (browsers pair again).
    let fresh = cert::regenerate_self_signed(&f.service, &names, Utc::now()).unwrap();
    assert_ne!(fresh.fingerprint(), first.fingerprint());
}

#[tokio::test]
async fn non_loopback_needs_tls_and_the_sandbox_needs_the_proxy() {
    let f = service();
    let lan = ServerOptions {
        addr: SocketAddr::from(([0, 0, 0, 0], 0)),
        ..loopback()
    };
    assert!(matches!(
        Server::bind_with(f.service.clone(), lan.clone(), TransportOptions::default()).await,
        Err(ServerError::TlsRequired(_))
    ));
    // `bind` stays loopback-only even with nothing else configured.
    assert!(matches!(
        Server::bind(f.service.clone(), lan).await,
        Err(ServerError::NotLoopback(_))
    ));
    assert!(matches!(
        Server::bind_with(
            f.service.clone(),
            loopback(),
            TransportOptions {
                sandbox: Some(SandboxOptions {
                    port: 0,
                    source: None
                }),
                ..TransportOptions::default()
            },
        )
        .await,
        Err(ServerError::SandboxNeedsProxy)
    ));
}

// ---------------------------------------------------------------------------
// Headers

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    let f = service();
    // Loopback `nail serve` (compat profile).
    let server = Server::bind(f.service.clone(), loopback()).await.unwrap();
    for path in ["/", "/api/nope", "/api/blob/xyz"] {
        let r = get(server.local_addr(), path, &[]).await;
        for (name, value) in [
            ("x-content-type-options", "nosniff"),
            ("referrer-policy", "no-referrer"),
            ("cross-origin-opener-policy", "same-origin"),
            ("cross-origin-embedder-policy", "require-corp"),
            ("x-frame-options", "DENY"),
            ("origin-agent-cluster", "?1"),
        ] {
            assert_eq!(r.header(name), Some(value), "{path}: {name}");
        }
        let csp = r.header("content-security-policy").unwrap();
        assert!(csp.contains("script-src 'self';"), "{path}");
        assert!(csp.contains("frame-ancestors 'none'"), "{path}");
        assert!(!csp.contains("require-trusted-types-for"), "{path}");
        assert!(
            r.header("content-security-policy-report-only")
                .unwrap()
                .contains("require-trusted-types-for 'script'")
        );
        assert!(r.header("permissions-policy").is_some());
        // No TLS, no HSTS.
        assert_eq!(r.header("strict-transport-security"), None, "{path}");
    }
    // Refusals carry them too.
    let refused = get(
        server.local_addr(),
        "/",
        &[
            ("Sec-Fetch-Site", "cross-site"),
            ("Sec-Fetch-Mode", "no-cors"),
            ("Sec-Fetch-Dest", "script"),
        ],
    )
    .await;
    assert_eq!(refused.status, 403);
    assert_eq!(refused.header("x-content-type-options"), Some("nosniff"));
    // The blob route keeps its own, stricter CSP.
    let hash = f.service.blobs().put(b"Subject: x\r\n\r\ny").unwrap();
    let blob = get(
        server.local_addr(),
        &format!("/api/blob/{}", hash.as_str()),
        &[],
    )
    .await;
    assert_eq!(blob.status, 200);
    assert_eq!(
        blob.header("content-security-policy"),
        Some("default-src 'none'; sandbox")
    );
    assert_eq!(
        blob.header("cross-origin-resource-policy"),
        Some("same-origin")
    );
}

// ---------------------------------------------------------------------------
// Sandbox origin and proxy

struct StubSource;

impl MailSource for StubSource {
    fn sanitized_html<'a>(&'a self, message_id: &'a str) -> HtmlLookup<'a> {
        let html = (message_id == "m-html")
            .then(|| r#"<p>Hello</p><img src="nail-proxy:TOKEN" alt="logo">"#.to_owned());
        Box::pin(async move { html })
    }

    /// Session `s-1` is the one whose cookie is `abc`.
    fn session_matches(&self, session_id: &str, headers: &axum::http::HeaderMap) -> bool {
        session_id == "s-1"
            && headers.get_all("cookie").iter().any(|v| {
                v.to_str()
                    .is_ok_and(|v| v.contains("__Host-nail-session=abc"))
            })
    }
}

struct Rig {
    server: Server,
    signer: Signer,
    upstream: Upstream,
    dns: Arc<ScriptedResolver>,
    fixture: Fixture,
}

async fn rig(source: Option<Arc<dyn MailSource>>) -> Rig {
    let f = service();
    let upstream = Upstream::start().await.unwrap();
    upstream.route(
        "/logo.png",
        Reply::ok("image/png", PNG).header("Set-Cookie", "tracker=1"),
    );
    upstream.route(
        "/page",
        Reply::ok("text/html", b"<script>x</script>".to_vec()),
    );
    let dns = ScriptedResolver::new();
    dns.set("img.example.org", &["127.0.0.1"]);
    dns.set("private.example.org", &["192.168.1.1"]);
    let tls = Arc::new(
        rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth(),
    );
    let signer = Signer::new(ProxyKey::generate());
    let fetcher = Fetcher::new(tls)
        .with_resolver(dns.clone())
        .allow_loopback_port_for_tests(upstream.port());
    let proxy = Arc::new(Proxy::new(signer.clone(), fetcher));
    let server = Server::bind_with(
        f.service.clone(),
        loopback(),
        TransportOptions {
            proxy: Some(proxy),
            sandbox: Some(SandboxOptions { port: 0, source }),
            headers: HeaderProfile::Strict,
            ..TransportOptions::default()
        },
    )
    .await
    .unwrap();
    Rig {
        server,
        signer,
        upstream,
        dns,
        fixture: f,
    }
}

#[tokio::test]
async fn mail_html_is_served_only_on_the_sandbox_origin() {
    let rig = rig(Some(Arc::new(StubSource))).await;
    let app = rig.server.local_addr();
    let sandbox = rig.server.sandbox_addr().unwrap();
    assert_ne!(app.port(), sandbox.port());
    let sandbox_origin = rig.server.sandbox_origin().unwrap();
    assert_eq!(sandbox_origin, format!("http://{sandbox}"));

    // The app may frame the sandbox origin (and only frames it).
    let page = get(app, "/", &[]).await;
    let csp = page.header("content-security-policy").unwrap().to_owned();
    assert!(
        csp.contains(&format!("frame-src 'self' blob: data: {sandbox_origin}")),
        "{csp}"
    );
    assert!(csp.contains("require-trusted-types-for 'script'"));

    let token = rig.signer.sign_render_for("m-html", "s-1").unwrap();
    let doc = get(
        sandbox,
        &format!("/render/{token}"),
        &[("Cookie", "__Host-nail-session=abc")],
    )
    .await;
    assert_eq!(doc.status, 200);
    assert_eq!(doc.header("content-type"), Some("text/html; charset=utf-8"));
    let body = String::from_utf8(doc.body.clone()).unwrap();
    assert!(body.contains("<p>Hello</p>"));
    assert!(body.contains(r#"src="/api/proxy/TOKEN""#), "{body}");
    let csp = doc.header("content-security-policy").unwrap();
    let sandbox_directive = csp
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("sandbox"))
        .unwrap();
    // P3 (ADR 0164): scripts run, but only the hash-pinned frame script,
    // and the document keeps an opaque origin.
    assert!(sandbox_directive.contains("allow-scripts"), "{csp}");
    assert!(!sandbox_directive.contains("allow-same-origin"), "{csp}");
    assert!(
        csp.contains(&format!(
            "script-src {};",
            nail_server::sandbox::frame_script_source()
        )),
        "{csp}"
    );
    assert!(body.contains(&format!(
        "<script>{}</script>",
        nail_server::sandbox::FRAME_SCRIPT
    )));
    // The height goes only to an app origin that may frame the document.
    let app_origin = format!("http://127.0.0.1:{}", app.port());
    let framed = get(
        sandbox,
        &format!("/render/{token}?parent={app_origin}"),
        &[("Cookie", "__Host-nail-session=abc")],
    )
    .await;
    assert_eq!(framed.status, 200);
    for parent in ["https://evil.example", "null", "*"] {
        assert_eq!(
            get(
                sandbox,
                &format!("/render/{token}?parent={parent}"),
                &[("Cookie", "__Host-nail-session=abc")],
            )
            .await
            .status,
            400,
            "{parent}"
        );
    }
    assert!(
        csp.contains(&format!("frame-ancestors http://127.0.0.1:{}", app.port())),
        "{csp}"
    );
    assert_eq!(
        doc.header("cross-origin-resource-policy"),
        Some("cross-origin")
    );
    assert_eq!(doc.header("cache-control"), Some("no-store"));
    assert_eq!(doc.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(doc.header("set-cookie"), None);

    // Bound to its session (M2b follow-up): without that session's
    // cookie, for another session, or bound to none — 404.
    assert_eq!(
        get(sandbox, &format!("/render/{token}"), &[]).await.status,
        404
    );
    assert_eq!(
        get(
            sandbox,
            &format!("/render/{token}"),
            &[("Cookie", "__Host-nail-session=other")]
        )
        .await
        .status,
        404
    );
    let elsewhere = rig.signer.sign_render_for("m-html", "s-2").unwrap();
    assert_eq!(
        get(
            sandbox,
            &format!("/render/{elsewhere}"),
            &[("Cookie", "__Host-nail-session=abc")]
        )
        .await
        .status,
        404
    );
    let unbound = rig.signer.sign_render("m-html").unwrap();
    assert_eq!(
        get(
            sandbox,
            &format!("/render/{unbound}"),
            &[("Cookie", "__Host-nail-session=abc")]
        )
        .await
        .status,
        404
    );

    // Tokens: other messages, other purposes, forgeries — all 404.
    let other = rig.signer.sign_render_for("m-unknown", "s-1").unwrap();
    assert_eq!(
        get(sandbox, &format!("/render/{other}"), &[]).await.status,
        404
    );
    let resource = rig
        .signer
        .sign_resource("m-html", "https://img.example.org/logo.png")
        .unwrap();
    assert_eq!(
        get(sandbox, &format!("/render/{resource}"), &[])
            .await
            .status,
        404
    );
    let foreign = Signer::new(ProxyKey::generate())
        .sign_render("m-html")
        .unwrap();
    assert_eq!(
        get(sandbox, &format!("/render/{foreign}"), &[])
            .await
            .status,
        404
    );
    assert_eq!(get(sandbox, "/render/m-html", &[]).await.status, 404);

    // The sandbox serves nothing of the app: no WebSocket, blobs, login,
    // bundle or test hooks.
    for path in [
        "/",
        "/index.html",
        "/api/ws",
        "/api/blob/abc",
        "/api/auth",
        "/api/test/echo",
    ] {
        let r = get(sandbox, path, &[]).await;
        assert_eq!(r.status, 404, "{path}");
    }
    // And the app does not serve mail documents.
    assert_eq!(get(app, &format!("/render/{token}"), &[]).await.status, 404);

    // Host checks (DNS rebinding) apply to the sandbox origin too.
    let evil = format!("evil.example:{}", sandbox.port());
    let stream = TcpStream::connect(sandbox).await.unwrap();
    let r = exchange(
        stream,
        "GET",
        &format!("/render/{token}"),
        &[("Host", &evil)],
    )
    .await;
    assert_eq!(r.status, 403);
}

#[tokio::test]
async fn the_app_refuses_requests_made_by_the_sandbox_origin() {
    let rig = rig(Some(Arc::new(StubSource))).await;
    let app = rig.server.local_addr();
    let sandbox_origin = rig.server.sandbox_origin().unwrap();
    let hash = rig
        .fixture
        .service
        .blobs()
        .put(b"Subject: secret\r\n\r\nbody")
        .unwrap();
    let blob = format!("/api/blob/{}", hash.as_str());

    // A mail document (opaque origin → cross-site) or anything on the
    // sandbox port (same-site) cannot read blobs or open the WebSocket.
    for (site, origin) in [
        ("same-site", sandbox_origin.as_str()),
        ("cross-site", "null"),
    ] {
        let r = get(
            app,
            &blob,
            &[
                ("Sec-Fetch-Site", site),
                ("Sec-Fetch-Mode", "cors"),
                ("Sec-Fetch-Dest", "empty"),
                ("Origin", origin),
            ],
        )
        .await;
        assert_eq!(r.status, 403, "{site}");
        assert!(!r.body.windows(6).any(|w| w == b"secret"));
    }
    let r = get(app, &blob, &[("Origin", &sandbox_origin)]).await;
    assert_eq!(r.status, 403, "the sandbox origin is not the app's origin");
    let r = get(
        app,
        "/api/ws",
        &[
            ("Origin", &sandbox_origin),
            ("Upgrade", "websocket"),
            ("Connection", "Upgrade"),
            ("Sec-WebSocket-Version", "13"),
            ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
        ],
    )
    .await;
    assert_eq!(r.status, 403);
    // The app itself still reads its blobs.
    let own = format!("http://{app}");
    let r = get(
        app,
        &blob,
        &[("Sec-Fetch-Site", "same-origin"), ("Origin", &own)],
    )
    .await;
    assert_eq!(r.status, 200);
}

#[tokio::test]
async fn the_proxy_route_fetches_only_signed_public_urls() {
    let rig = rig(None).await;
    let up = rig.upstream.port();
    let url = format!("http://img.example.org:{up}/logo.png");
    let token = rig.signer.sign_resource("m-1", &url).unwrap();

    for addr in [rig.server.local_addr(), rig.server.sandbox_addr().unwrap()] {
        let r = get(
            addr,
            &format!("/api/proxy/{token}"),
            &[
                ("Cookie", "__Host-nail-session=secret-session"),
                ("Authorization", "Bearer secret"),
                ("Sec-Fetch-Site", "cross-site"),
                ("Sec-Fetch-Mode", "no-cors"),
                ("Sec-Fetch-Dest", "image"),
                ("Origin", "null"),
            ],
        )
        .await;
        assert_eq!(r.status, 200, "{addr}");
        assert_eq!(r.body, PNG);
        assert_eq!(r.header("content-type"), Some("image/png"));
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
        assert!(
            r.header("content-security-policy")
                .unwrap()
                .contains("sandbox")
        );
        assert_eq!(
            r.header("cross-origin-resource-policy"),
            Some("cross-origin")
        );
        assert!(
            r.header("cache-control")
                .unwrap()
                .starts_with("private, max-age=")
        );
        assert_eq!(r.header("set-cookie"), None, "upstream cookies are dropped");
    }
    // Nothing of the browser's request reached the upstream.
    for request in rig.upstream.requests() {
        assert_eq!(request.header("cookie"), None);
        assert_eq!(request.header("authorization"), None);
        assert_eq!(request.header("referer"), None);
        assert_eq!(request.header("origin"), None);
    }

    let app = rig.server.local_addr();
    // Never an open proxy: raw URLs and forged tokens are 404.
    for path in [
        format!("/api/proxy/{}", url.replace('/', "%2F")),
        "/api/proxy/http:%2F%2F127.0.0.1%2F".to_owned(),
        format!(
            "/api/proxy/{}",
            Signer::new(ProxyKey::generate())
                .sign_resource("m-1", &url)
                .unwrap()
        ),
        format!("/api/proxy/{}", rig.signer.sign_render("m-1").unwrap()),
    ] {
        assert_eq!(get(app, &path, &[]).await.status, 404, "{path}");
    }
    // Signed, but the name now resolves privately: refused, no connection.
    let before = rig.upstream.connections();
    let private = rig
        .signer
        .sign_resource("m-1", &format!("http://private.example.org:{up}/logo.png"))
        .unwrap();
    assert_eq!(
        get(app, &format!("/api/proxy/{private}"), &[]).await.status,
        403
    );
    rig.dns.set("img.example.org", &["169.254.169.254"]);
    assert_eq!(
        get(app, &format!("/api/proxy/{token}"), &[]).await.status,
        403
    );
    assert_eq!(rig.upstream.connections(), before);
    // Disallowed types are refused.
    rig.dns.set("img.example.org", &["127.0.0.1"]);
    let page = rig
        .signer
        .sign_resource("m-1", &format!("http://img.example.org:{up}/page"))
        .unwrap();
    assert_eq!(
        get(app, &format!("/api/proxy/{page}"), &[]).await.status,
        415
    );
    // Host checks still apply to the proxy route.
    let stream = TcpStream::connect(app).await.unwrap();
    let evil = format!("evil.example:{}", app.port());
    let r = exchange(
        stream,
        "GET",
        &format!("/api/proxy/{token}"),
        &[("Host", &evil)],
    )
    .await;
    assert_eq!(r.status, 403);
}

#[tokio::test]
async fn without_a_proxy_there_is_no_proxy_route() {
    let f = service();
    let server = Server::bind(f.service.clone(), loopback()).await.unwrap();
    let token = Signer::new(ProxyKey::generate())
        .sign_resource("m-1", "https://img.example.org/a.png")
        .unwrap();
    assert_eq!(
        get(server.local_addr(), &format!("/api/proxy/{token}"), &[])
            .await
            .status,
        404
    );
    assert_eq!(server.sandbox_addr(), None);
    assert_eq!(server.sandbox_origin(), None);
}

#[tokio::test]
async fn the_service_is_the_default_mail_source() {
    let rig = rig(None).await;
    let sandbox = rig.server.sandbox_addr().unwrap();
    // Unknown message: nothing to render.
    let token = rig
        .signer
        .sign_render_for("no-such-message", "rsess-1")
        .unwrap();
    assert_eq!(
        get(sandbox, &format!("/render/{token}"), &[]).await.status,
        404
    );
    // A demo message with HTML, but no live remote session behind the
    // token: the service refuses it (the session binding is tested with
    // real sessions in nail-core's remote tests).
    let id = demo_html_message(&rig.fixture.service)
        .await
        .expect("the demo corpus has an HTML message");
    let token = rig.signer.sign_render_for(&id, "rsess-1").unwrap();
    let r = get(
        sandbox,
        &format!("/render/{token}"),
        &[("Cookie", "__Host-nail-session=abc")],
    )
    .await;
    assert_eq!(r.status, 404);
}

/// The id of a seeded demo message that has an HTML body, if any.
async fn demo_html_message(service: &Service) -> Option<String> {
    use nail_api::{Request, Response};
    let Response::Accounts { accounts } = service.handle(Request::ListAccounts).await else {
        return None;
    };
    for account in accounts {
        let Response::Folders { folders } = service
            .handle(Request::ListFolders {
                account_id: account.id.clone(),
            })
            .await
        else {
            continue;
        };
        for folder in folders {
            let Response::Messages { messages, .. } = service
                .handle(Request::ListMessages {
                    folder_id: folder.id.clone(),
                    offset: 0,
                    limit: 200,
                })
                .await
            else {
                continue;
            };
            for message in messages {
                if let Response::Message { message } = service
                    .handle(Request::GetMessage {
                        id: message.id.clone(),
                    })
                    .await
                    && message.sanitized_html.is_some()
                {
                    return Some(message.id.to_string());
                }
            }
        }
    }
    None
}
