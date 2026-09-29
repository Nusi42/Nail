//! Discovery against the mock HTTP and DNS servers of `nail-testkit-net`:
//! real `reqwest` and `hickory-resolver` code paths, no network.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{
    AuthConfig, DiscoveredAccount, DiscoverySource, DiscoveryWarning, IncomingServer, TlsMode,
};
use nail_discovery::{Discoverer, DiscoveryError, HickoryDns, ReqwestFetch, TcpProbe, discover};
use nail_testkit_net::{DnsZone, MockDnsServer, MockHttpServer, MockResponse};

const AUTOCONFIG: &str = r#"<clientConfig version="1.1"><emailProvider id="example.org">
  <displayName>Example Mail</displayName>
  <incomingServer type="imap"><hostname>imap.example.org</hostname><port>993</port><socketType>SSL</socketType>
    <username>%EMAILADDRESS%</username><authentication>password-cleartext</authentication></incomingServer>
  <outgoingServer type="smtp"><hostname>smtp.example.org</hostname><port>465</port><socketType>SSL</socketType>
    <username>%EMAILADDRESS%</username><authentication>password-cleartext</authentication></outgoingServer>
</emailProvider></clientConfig>"#;

const GMAIL_ISPDB: &str = r#"<clientConfig version="1.1"><emailProvider id="googlemail.com">
  <displayName>Google Mail</displayName>
  <incomingServer type="imap"><hostname>imap.gmail.com</hostname><port>993</port><socketType>SSL</socketType>
    <username>%EMAILADDRESS%</username><authentication>OAuth2</authentication><authentication>password-cleartext</authentication></incomingServer>
  <outgoingServer type="smtp"><hostname>smtp.gmail.com</hostname><port>465</port><socketType>SSL</socketType>
    <username>%EMAILADDRESS%</username><authentication>OAuth2</authentication></outgoingServer>
</emailProvider></clientConfig>"#;

const AUTODISCOVER: &str = r#"<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006">
<Response xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a"><Account>
<Action>settings</Action>
<Protocol><Type>IMAP</Type><Server>imap.corp.example</Server><Port>993</Port><SSL>on</SSL></Protocol>
<Protocol><Type>SMTP</Type><Server>smtp.corp.example</Server><Port>587</Port><Encryption>TLS</Encryption></Protocol>
</Account></Response></Autodiscover>"#;

struct Mocks {
    http: MockHttpServer,
    dns: MockDnsServer,
}

async fn mocks(zone: DnsZone) -> Mocks {
    Mocks {
        http: MockHttpServer::start().await.unwrap(),
        dns: MockDnsServer::start(zone).await.unwrap(),
    }
}

fn discoverer(m: &Mocks, probes: HashMap<(String, u16), std::net::SocketAddr>) -> Discoverer {
    let client = nail_auth::http_client_builder(nail_auth::tls_config_with_roots(&[]).unwrap())
        .no_proxy()
        .build()
        .unwrap();
    Discoverer::new(
        Arc::new(ReqwestFetch::new(client).route_all_to(m.http.addr())),
        Arc::new(HickoryDns::with_name_server(m.dns.addr(), Duration::from_secs(2)).unwrap()),
        Arc::new(TcpProbe::with_overrides(Duration::from_secs(2), probes)),
    )
    .with_timeout(Duration::from_secs(5))
}

fn hosts(found: &[DiscoveredAccount]) -> Vec<(DiscoverySource, String, Option<String>)> {
    found
        .iter()
        .map(|c| {
            (
                c.source,
                c.incoming.server().host.clone(),
                c.outgoing.as_ref().map(|o| o.server().host.clone()),
            )
        })
        .collect()
}

#[tokio::test]
async fn autoconfig_subdomain() {
    let m = mocks(DnsZone::new()).await;
    m.http.get(
        Some("autoconfig.example.org"),
        "/mail/config-v1.1.xml",
        MockResponse::xml(AUTOCONFIG),
    );
    let found = discover("ada@example.org", &discoverer(&m, HashMap::new()))
        .await
        .unwrap();
    assert_eq!(
        hosts(&found),
        [(
            DiscoverySource::Autoconfig,
            "imap.example.org".to_owned(),
            Some("smtp.example.org".to_owned())
        )]
    );
    assert_eq!(found[0].provider_name.as_deref(), Some("Example Mail"));
    assert!(
        matches!(&found[0].incoming.server().auth, AuthConfig::Password { username, secret: None, .. } if username == "ada@example.org")
    );
    let request = m
        .http
        .requests()
        .into_iter()
        .find(|r| r.host == "autoconfig.example.org")
        .unwrap();
    assert_eq!(
        request.query.as_deref(),
        Some("emailaddress=ada%40example.org")
    );
    // Every other source was asked too.
    let asked: Vec<String> = m
        .http
        .requests()
        .iter()
        .map(|r| format!("{} {}{}", r.method, r.host, r.path))
        .collect();
    for expected in [
        "GET example.org/.well-known/autoconfig/mail/config-v1.1.xml",
        "GET autoconfig.thunderbird.net/v1.1/example.org",
        "POST autodiscover.example.org/autodiscover/autodiscover.xml",
    ] {
        assert!(
            asked.iter().any(|a| a == expected),
            "{expected} not in {asked:?}"
        );
    }
    let dns: Vec<String> = m
        .dns
        .queries()
        .into_iter()
        .map(|(t, n)| format!("{t} {n}"))
        .collect();
    assert!(
        dns.contains(&"SRV _imaps._tcp.example.org".to_owned()),
        "{dns:?}"
    );
    assert!(dns.contains(&"SRV _submissions._tcp.example.org".to_owned()));
}

#[tokio::test]
async fn well_known_and_redirects() {
    let m = mocks(DnsZone::new()).await;
    m.http.get(
        Some("example.org"),
        "/.well-known/autoconfig/mail/config-v1.1.xml",
        MockResponse::status(301).with_header("Location", "https://config.example.net/ac.xml"),
    );
    m.http.get(
        Some("config.example.net"),
        "/ac.xml",
        MockResponse::xml(AUTOCONFIG),
    );
    // A redirect to plain HTTP is not followed.
    m.http.get(
        Some("autoconfig.example.org"),
        "/mail/config-v1.1.xml",
        MockResponse::status(302).with_header("Location", "http://evil.example/ac.xml"),
    );
    m.http.get(
        Some("evil.example"),
        "/ac.xml",
        MockResponse::xml(GMAIL_ISPDB),
    );
    let found = discover("ada@example.org", &discoverer(&m, HashMap::new()))
        .await
        .unwrap();
    assert_eq!(hosts(&found).len(), 1);
    assert_eq!(found[0].source, DiscoverySource::WellKnown);
    assert!(!m.http.requests().iter().any(|r| r.host == "evil.example"));
}

#[tokio::test]
async fn ispdb_by_domain_and_by_mx() {
    let mut zone = DnsZone::new();
    zone.add_mx("custom.example", 10, "aspmx.l.google.com")
        .add_mx("custom.example", 20, "alt1.aspmx.l.google.com");
    let m = mocks(zone).await;
    m.http.get(
        Some("autoconfig.thunderbird.net"),
        "/v1.1/google.com",
        MockResponse::xml(GMAIL_ISPDB),
    );
    m.http.get(
        Some("autoconfig.thunderbird.net"),
        "/v1.1/gmail.com",
        MockResponse::xml(GMAIL_ISPDB),
    );

    let d = discoverer(&m, HashMap::new());
    let found = discover("ada@gmail.com", &d).await.unwrap();
    assert_eq!(found[0].source, DiscoverySource::Ispdb);
    assert!(
        matches!(&found[0].incoming.server().auth, AuthConfig::OAuth2 { provider, .. } if provider == "google")
    );

    // A custom domain hosted by Google: found through its MX host.
    let found = discover("ada@custom.example", &d).await.unwrap();
    assert_eq!(
        hosts(&found),
        [(
            DiscoverySource::Ispdb,
            "imap.gmail.com".to_owned(),
            Some("smtp.gmail.com".to_owned())
        )]
    );
    assert!(
        matches!(&found[0].incoming.server().auth, AuthConfig::OAuth2 { username, .. } if username == "ada@custom.example")
    );
}

#[tokio::test]
async fn srv_records() {
    let mut zone = DnsZone::new();
    zone.add_srv("_imaps._tcp.example.org", 10, 0, 993, "imap-b.example.org")
        .add_srv("_imaps._tcp.example.org", 5, 0, 993, "imap-a.example.org")
        .add_srv("_pop3s._tcp.example.org", 0, 0, 0, ".")
        .add_srv(
            "_submission._tcp.example.org",
            0,
            1,
            587,
            "smtp.example.org",
        );
    let m = mocks(zone).await;
    let found = discover("ada@example.org", &discoverer(&m, HashMap::new()))
        .await
        .unwrap();
    assert_eq!(
        hosts(&found),
        [(
            DiscoverySource::Srv,
            "imap-a.example.org".to_owned(),
            Some("smtp.example.org".to_owned())
        )]
    );
    let smtp = found[0].outgoing.as_ref().unwrap().server();
    assert_eq!((smtp.port, smtp.tls), (587, TlsMode::Starttls));
    assert_eq!(found[0].incoming.server().tls, TlsMode::Implicit);
}

#[tokio::test]
async fn autodiscover_with_redirects() {
    let m = mocks(DnsZone::new()).await;
    let redirect_addr = r#"<Autodiscover><Response><Account><Action>redirectAddr</Action>
        <RedirectAddr>ada@corp.example</RedirectAddr></Account></Response></Autodiscover>"#;
    let redirect_url = r#"<Autodiscover><Response><Account><Action>redirectUrl</Action>
        <RedirectUrl>https://ad.corp.example/autodiscover/autodiscover.xml</RedirectUrl></Account></Response></Autodiscover>"#;
    m.http.post(
        Some("autodiscover.example.org"),
        "/autodiscover/autodiscover.xml",
        MockResponse::xml(redirect_addr),
    );
    m.http.post(
        Some("autodiscover.corp.example"),
        "/autodiscover/autodiscover.xml",
        MockResponse::xml(redirect_url),
    );
    m.http.post(
        Some("ad.corp.example"),
        "/autodiscover/autodiscover.xml",
        MockResponse::xml(AUTODISCOVER),
    );
    let found = discover("ada@example.org", &discoverer(&m, HashMap::new()))
        .await
        .unwrap();
    assert_eq!(
        hosts(&found),
        [(
            DiscoverySource::Autodiscover,
            "imap.corp.example".to_owned(),
            Some("smtp.corp.example".to_owned())
        )]
    );
    let posts: Vec<_> = m
        .http
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST")
        .collect();
    let last = posts.iter().find(|r| r.host == "ad.corp.example").unwrap();
    let body = String::from_utf8(last.body.clone()).unwrap();
    assert!(
        body.contains("<EMailAddress>ada@corp.example</EMailAddress>"),
        "{body}"
    );
}

#[tokio::test]
async fn guessing_only_when_nothing_else_answers() {
    let m = mocks(DnsZone::new()).await;
    let imap = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let smtp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let probes = HashMap::from([
        (
            ("mail.example.org".to_owned(), 993),
            imap.local_addr().unwrap(),
        ),
        (
            ("smtp.example.org".to_owned(), 587),
            smtp.local_addr().unwrap(),
        ),
    ]);
    let found = discover("ada@example.org", &discoverer(&m, probes.clone()))
        .await
        .unwrap();
    assert_eq!(
        hosts(&found),
        [(
            DiscoverySource::Guess,
            "mail.example.org".to_owned(),
            Some("smtp.example.org".to_owned())
        )]
    );
    let IncomingServer::Imap { server, .. } = &found[0].incoming else {
        panic!("expected IMAP")
    };
    assert_eq!((server.port, server.tls), (993, TlsMode::Implicit));
    assert_eq!(
        found[0].outgoing.as_ref().unwrap().server().tls,
        TlsMode::Starttls
    );

    // With a real answer, no guessing.
    m.http.get(
        Some("autoconfig.example.org"),
        "/mail/config-v1.1.xml",
        MockResponse::xml(AUTOCONFIG),
    );
    let found = discover("ada@example.org", &discoverer(&m, probes.clone()))
        .await
        .unwrap();
    assert!(found.iter().all(|c| c.source != DiscoverySource::Guess));

    let nothing = mocks(DnsZone::new()).await;
    let d = discoverer(&nothing, probes).with_guessing(false);
    assert!(discover("ada@example.org", &d).await.unwrap().is_empty());
}

#[tokio::test]
async fn slow_sources_do_not_block_the_others() {
    let mut zone = DnsZone::new();
    zone.add_srv("_imaps._tcp.example.org", 0, 0, 993, "imap.example.org");
    let m = mocks(zone).await;
    m.http.get(
        Some("autoconfig.example.org"),
        "/mail/config-v1.1.xml",
        MockResponse::xml(AUTOCONFIG).with_delay(Duration::from_secs(10)),
    );
    let d = discoverer(&m, HashMap::new()).with_timeout(Duration::from_millis(1500));
    let started = std::time::Instant::now();
    let found = discover("ada@example.org", &d).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        hosts(&found),
        [(DiscoverySource::Srv, "imap.example.org".to_owned(), None)]
    );
}

#[tokio::test]
async fn duplicates_across_sources_keep_the_best_source() {
    let mut zone = DnsZone::new();
    zone.add_srv("_imaps._tcp.example.org", 0, 0, 993, "imap.example.org")
        .add_srv(
            "_submissions._tcp.example.org",
            0,
            0,
            465,
            "smtp.example.org",
        );
    let m = mocks(zone).await;
    m.http.get(
        Some("autoconfig.thunderbird.net"),
        "/v1.1/example.org",
        MockResponse::xml(AUTOCONFIG),
    );
    let found = discover("ada@example.org", &discoverer(&m, HashMap::new()))
        .await
        .unwrap();
    // Same servers and logins from the ISPDB and SRV: listed once.
    assert_eq!(
        hosts(&found),
        [(
            DiscoverySource::Ispdb,
            "imap.example.org".to_owned(),
            Some("smtp.example.org".to_owned())
        )]
    );
}

#[tokio::test]
async fn invalid_addresses() {
    let m = mocks(DnsZone::new()).await;
    let err = discover("not-an-address", &discoverer(&m, HashMap::new()))
        .await
        .unwrap_err();
    assert!(matches!(err, DiscoveryError::InvalidEmail(_)));
    assert!(m.http.requests().is_empty());
}

const FORGED: &str = r#"<clientConfig version="1.1"><emailProvider id="example.org">
  <displayName>Example Mail (really)</displayName>
  <incomingServer type="imap"><hostname>imap.attacker.example</hostname><port>993</port><socketType>SSL</socketType>
    <username>%EMAILADDRESS%</username><authentication>password-cleartext</authentication></incomingServer>
  <outgoingServer type="smtp"><hostname>smtp.attacker.example</hostname><port>465</port><socketType>SSL</socketType>
    <username>%EMAILADDRESS%</username><authentication>password-cleartext</authentication></outgoingServer>
</emailProvider></clientConfig>"#;

/// The mock HTTP server, except that every `https` request fails (a domain
/// without an HTTPS autoconfig host): only the plain-HTTP fallback answers.
#[derive(Debug)]
struct HttpOnly(ReqwestFetch);

impl nail_discovery::HttpFetch for HttpOnly {
    fn get<'a>(
        &'a self,
        url: &'a url::Url,
    ) -> nail_api::engine::BoxFuture<'a, Result<nail_discovery::HttpReply, nail_discovery::NetError>>
    {
        Box::pin(async move {
            if url.scheme() == "https" && url.host_str() != Some("autoconfig.thunderbird.net") {
                return Err(nail_discovery::NetError("no HTTPS here".into()));
            }
            self.0.get(url).await
        })
    }

    fn post<'a>(
        &'a self,
        url: &'a url::Url,
        content_type: &'a str,
        body: Vec<u8>,
    ) -> nail_api::engine::BoxFuture<'a, Result<nail_discovery::HttpReply, nail_discovery::NetError>>
    {
        self.0.post(url, content_type, body)
    }
}

fn http_only_discoverer(m: &Mocks) -> Discoverer {
    let client = nail_auth::http_client_builder(nail_auth::tls_config_with_roots(&[]).unwrap())
        .no_proxy()
        .build()
        .unwrap();
    Discoverer::new(
        Arc::new(HttpOnly(
            ReqwestFetch::new(client).route_all_to(m.http.addr()),
        )),
        Arc::new(HickoryDns::with_name_server(m.dns.addr(), Duration::from_secs(2)).unwrap()),
        Arc::new(TcpProbe::with_overrides(
            Duration::from_secs(2),
            HashMap::new(),
        )),
    )
    .with_timeout(Duration::from_secs(5))
}

/// Review M1a: an autoconfig answer fetched over plain HTTP can be forged
/// by anyone on the network; it must never outrank an authenticated or
/// DNS answer, and it carries warnings for the setup form.
#[tokio::test]
async fn plain_http_answers_rank_last_and_carry_warnings() {
    let mut zone = DnsZone::new();
    zone.add_srv("_imaps._tcp.example.org", 0, 0, 993, "imap.example.org")
        .add_mx("example.org", 10, "mx1.mailhost.example");
    let m = mocks(zone).await;
    m.http.get(
        Some("autoconfig.example.org"),
        "/mail/config-v1.1.xml",
        MockResponse::xml(FORGED),
    );
    let found = discover("ada@example.org", &http_only_discoverer(&m))
        .await
        .unwrap();
    assert_eq!(
        hosts(&found),
        [
            (DiscoverySource::Srv, "imap.example.org".to_owned(), None),
            (
                DiscoverySource::Autoconfig,
                "imap.attacker.example".to_owned(),
                Some("smtp.attacker.example".to_owned())
            ),
        ]
    );
    assert!(found[0].warnings.is_empty(), "{:?}", found[0].warnings);
    assert_eq!(
        found[1].warnings,
        [
            DiscoveryWarning::InsecureSource,
            DiscoveryWarning::ForeignHosts
        ]
    );

    // Hosts under the MX domain are not foreign.
    m.http.get(
        Some("autoconfig.example.org"),
        "/mail/config-v1.1.xml",
        MockResponse::xml(FORGED.replace("attacker.example", "mailhost.example")),
    );
    let found = discover("ada@example.org", &http_only_discoverer(&m))
        .await
        .unwrap();
    assert_eq!(found[1].warnings, [DiscoveryWarning::InsecureSource]);

    // When an authenticated HTTPS source answers, the HTTP answer is gone.
    m.http.get(
        Some("autoconfig.thunderbird.net"),
        "/v1.1/example.org",
        MockResponse::xml(AUTOCONFIG),
    );
    let found = discover("ada@example.org", &http_only_discoverer(&m))
        .await
        .unwrap();
    assert_eq!(found[0].source, DiscoverySource::Ispdb);
    assert!(
        found
            .iter()
            .all(|c| !c.warnings.contains(&DiscoveryWarning::InsecureSource)),
        "{found:?}"
    );
}
