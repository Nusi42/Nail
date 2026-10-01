//! Autodiscover end to end against the loopback mock server (several fake
//! hosts behind one 127.0.0.1 port): engine choice, Windows sign-in and
//! Basic, redirects, and — above all — that credentials never reach a
//! host outside the address's domain without the user's confirmation.

use std::collections::BTreeMap;
use std::sync::Arc;

use nail_api::ErrorCode;
use nail_api::accounts::{AuthConfig, DiscoveryWarning, IncomingServer, MailProtocol};
use nail_api::engine::{BoxFuture, EngineError};
use nail_api::exchange::ExchangeEngine;
use nail_api::exchange::engine::{NegotiateAuth, NegotiateSession, NegotiateStep};
use nail_discovery::{DnsLookup, MxRecord, NetError, SrvRecord};
use nail_exchange::autodiscover::{
    AutodiscoverError, AutodiscoverOutcome, AutodiscoverRequest, Autodiscoverer, ExchangeDiscovery,
    RedirectReason,
};
use nail_exchange::http::ReqwestHttp;
use nail_exchange::negotiate::{AuthScheme, HttpLogin, Password, SignInError, WindowsLogin};
use nail_exchange::testing::{
    AUTHENTICATE_TOKEN, CHALLENGE_TOKEN, MockAutodiscoverServer, MockHost, MockLogin, MockPox,
    MockPoxSettings, MockV2, NEGOTIATE_TOKEN,
};

const ONLINE_EWS: &str = "https://outlook.office365.com/EWS/Exchange.asmx";
const CORP_EWS: &str = "https://mail.corp.example/EWS/Exchange.asmx";
const GLOBAL: &str = "autodiscover-s.outlook.com";

#[derive(Debug)]
struct FakeSession(u8);

impl NegotiateSession for FakeSession {
    fn step(&mut self, server: Option<&[u8]>) -> Result<NegotiateStep, EngineError> {
        self.0 += 1;
        match (self.0, server) {
            (1, None) => Ok(NegotiateStep::Continue(NEGOTIATE_TOKEN.to_vec())),
            (2, Some(t)) if t == CHALLENGE_TOKEN => {
                Ok(NegotiateStep::Done(Some(AUTHENTICATE_TOKEN.to_vec())))
            }
            _ => Err(EngineError::permanent(ErrorCode::AuthFailed, "unexpected")),
        }
    }
}

#[derive(Debug, Default)]
struct FakeNegotiate {
    spns: std::sync::Mutex<Vec<String>>,
}

impl NegotiateAuth for FakeNegotiate {
    fn start(&self, spn: &str) -> Result<Box<dyn NegotiateSession>, EngineError> {
        self.spns.lock().unwrap().push(spn.to_owned());
        Ok(Box::new(FakeSession(0)))
    }
}

#[derive(Debug, Default)]
struct FakeDns(Vec<SrvRecord>);

impl DnsLookup for FakeDns {
    fn srv<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<SrvRecord>, NetError>> {
        let records = if name == "_autodiscover._tcp.corp.example" {
            self.0.clone()
        } else {
            Vec::new()
        };
        Box::pin(async move { Ok(records) })
    }

    fn mx<'a>(&'a self, _domain: &'a str) -> BoxFuture<'a, Result<Vec<MxRecord>, NetError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

async fn server(hosts: &[(&str, MockHost)]) -> MockAutodiscoverServer {
    let map: BTreeMap<String, MockHost> = hosts
        .iter()
        .map(|(h, c)| ((*h).to_owned(), c.clone()))
        .collect();
    MockAutodiscoverServer::start_hosts(map).await.unwrap()
}

fn discoverer(server: &MockAutodiscoverServer, online: bool) -> Autodiscoverer {
    let http = ReqwestHttp::system().unwrap().route_all_to(server.addr());
    Autodiscoverer::new(Arc::new(http)).with_online_host(online.then(|| GLOBAL.to_owned()))
}

fn request(address: &str, login: HttpLogin) -> AutodiscoverRequest {
    AutodiscoverRequest {
        address: address.into(),
        login,
        ..AutodiscoverRequest::default()
    }
}

fn windows() -> HttpLogin {
    HttpLogin::Windows(WindowsLogin::new(Arc::new(FakeNegotiate::default())))
}

fn basic(password: &str) -> HttpLogin {
    HttpLogin::Basic {
        username: "CORP\\ada".into(),
        password: Password::new(password),
    }
}

fn basic_host(pox: MockPox) -> MockHost {
    MockHost {
        pox: Some(pox),
        login: Some(MockLogin::Basic {
            username: "CORP\\ada".into(),
            password: "secret".into(),
        }),
        ..MockHost::default()
    }
}

fn corp_settings() -> MockPoxSettings {
    MockPoxSettings {
        display_name: Some("Ada Lovelace".into()),
        internal_ews_url: Some("https://ex01.corp.local/EWS/Exchange.asmx".into()),
        external_ews_url: Some(CORP_EWS.into()),
        imap: Some(("imap.corp.example".into(), 993)),
        smtp: Some(("smtp.corp.example".into(), 587)),
        alternative_mailboxes: vec![(
            "Delegate".into(),
            "Support".into(),
            "support@corp.example".into(),
        )],
    }
}

fn found(outcome: AutodiscoverOutcome) -> ExchangeDiscovery {
    match outcome {
        AutodiscoverOutcome::Found(found) => *found,
        other => panic!("expected settings, got {other:?}"),
    }
}

#[tokio::test]
async fn exchange_online_gets_graph_without_credentials() {
    let server = server(&[(
        GLOBAL,
        MockHost {
            v2: Some(MockV2::Url(ONLINE_EWS.into())),
            ..MockHost::default()
        },
    )])
    .await;
    let outcome = discoverer(&server, true)
        .discover(&request("Ada@Contoso.Example", windows()))
        .await
        .unwrap();
    let found = found(outcome);
    assert_eq!(found.choice.engine, ExchangeEngine::Graph);
    assert!(found.choice.online);
    assert_eq!(found.address, "Ada@contoso.example");
    assert_eq!(found.candidates.len(), 1);
    let IncomingServer::Exchange {
        server: endpoint,
        options,
    } = &found.candidates[0].incoming
    else {
        panic!("an Exchange proposal");
    };
    assert_eq!(options.engine, ExchangeEngine::Graph);
    assert!(
        matches!(&endpoint.auth, AuthConfig::OAuth2 { provider, .. } if provider == "microsoft")
    );
    assert_eq!(found.localized().id, "exchange-autodiscover-online");
    // The domain's own hosts were asked first; nobody got credentials.
    let hosts: Vec<String> = server.requests().into_iter().map(|r| r.host).collect();
    assert_eq!(
        hosts,
        ["autodiscover.contoso.example", "contoso.example", GLOBAL]
    );
    assert!(server.hosts_with_credentials().is_empty());

    // Without Microsoft's endpoint nothing answers.
    let outcome = discoverer(&server, false)
        .discover(&request("ada@contoso.example", HttpLogin::None))
        .await
        .unwrap();
    assert!(matches!(outcome, AutodiscoverOutcome::NotFound { .. }));
}

#[tokio::test]
async fn on_premises_via_v2_with_windows_sign_in() {
    let server = server(&[
        (
            "autodiscover.corp.example",
            MockHost {
                v2: Some(MockV2::Url(CORP_EWS.into())),
                ..MockHost::default()
            },
        ),
        (
            "mail.corp.example",
            MockHost {
                pox: Some(MockPox::Settings(corp_settings())),
                login: Some(MockLogin::Negotiate),
                ..MockHost::default()
            },
        ),
    ])
    .await;
    let negotiate = Arc::new(FakeNegotiate::default());
    let login = HttpLogin::Windows(WindowsLogin::new(negotiate.clone()));
    let found = found(
        discoverer(&server, true)
            .discover(&request("ada@corp.example", login))
            .await
            .unwrap(),
    );
    assert_eq!(found.choice.engine, ExchangeEngine::Ews);
    assert_eq!(found.choice.ews_url.as_deref(), Some(CORP_EWS));
    assert_eq!(found.display_name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(found.mailboxes.len(), 1);
    assert_eq!(found.mailboxes[0].address, "support@corp.example");
    let IncomingServer::Exchange {
        server: endpoint,
        options,
    } = &found.candidates[0].incoming
    else {
        panic!()
    };
    assert_eq!(endpoint.host, "mail.corp.example");
    assert_eq!(options.url, None, "the default path");
    assert!(matches!(
        endpoint.auth,
        AuthConfig::Windows { username: None, .. }
    ));
    assert!(options.issues(endpoint).is_empty());
    // The IMAP fallback from the POX answer.
    assert_eq!(found.candidates[1].incoming.protocol(), MailProtocol::Imap);
    assert!(found.candidates.iter().all(|c| c.warnings.is_empty()));
    assert_eq!(*negotiate.spns.lock().unwrap(), ["HTTP/mail.corp.example"]);
    assert_eq!(server.hosts_with_credentials(), ["mail.corp.example"]);
    // The global endpoint was not needed.
    assert!(server.requests().iter().all(|r| r.host != GLOBAL));
}

#[tokio::test]
async fn pox_with_basic_and_login_prompts() {
    let server = server(&[(
        "corp.example",
        basic_host(MockPox::Settings(corp_settings())),
    )])
    .await;
    let d = discoverer(&server, false);
    let found = found(
        d.discover(&request("ada@corp.example", basic("secret")))
            .await
            .unwrap(),
    );
    assert_eq!(found.choice.engine, ExchangeEngine::Ews);
    assert!(matches!(&found.candidates[0].incoming.server().auth,
        AuthConfig::Password { username, .. } if username == "CORP\\ada"));
    assert!(
        found
            .trace
            .iter()
            .any(|u| u.contains("corp.example/autodiscover/autodiscover.xml"))
    );

    let error = d
        .discover(&request("ada@corp.example", basic("wrong")))
        .await
        .unwrap_err();
    assert_eq!(error, AutodiscoverError::SignIn(SignInError::Rejected));
    assert_eq!(error.code(), ErrorCode::AuthFailed);

    let outcome = d
        .discover(&request("ada@corp.example", HttpLogin::None))
        .await
        .unwrap();
    let AutodiscoverOutcome::NeedsLogin(needed) = outcome else {
        panic!("a login prompt, got {outcome:?}");
    };
    assert_eq!(needed.host, "corp.example");
    assert_eq!(needed.schemes, [AuthScheme::Basic]);
    assert_eq!(needed.localized().id, "exchange-autodiscover-needs-login");

    // The login goes only to the hosts the user answered a prompt for.
    let mut limited = request("ada@corp.example", basic("secret"));
    limited.login_hosts = Some(vec!["autodiscover.corp.example".into()]);
    let outcome = d.discover(&limited).await.unwrap();
    let AutodiscoverOutcome::NeedsLogin(needed) = outcome else {
        panic!("a login prompt for the other host, got {outcome:?}");
    };
    assert_eq!(needed.host, "corp.example");
    limited.login_hosts = Some(vec!["corp.example".into()]);
    let outcome = d.discover(&limited).await.unwrap();
    assert!(
        matches!(outcome, AutodiscoverOutcome::Found(_)),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn foreign_redirects_need_confirmation() {
    let target = "https://mail.hoster.example/autodiscover/autodiscover.xml";
    let server = server(&[
        (
            "corp.example",
            MockHost {
                pox: Some(MockPox::HttpRedirect(target.into())),
                ..MockHost::default()
            },
        ),
        (
            "mail.hoster.example",
            basic_host(MockPox::Settings(MockPoxSettings {
                external_ews_url: Some("https://mail.hoster.example/EWS/Exchange.asmx".into()),
                ..MockPoxSettings::default()
            })),
        ),
    ])
    .await;
    let d = discoverer(&server, false);
    let outcome = d
        .discover(&request("ada@corp.example", basic("secret")))
        .await
        .unwrap();
    let AutodiscoverOutcome::ConfirmRedirect(confirm) = outcome else {
        panic!("a confirmation, got {outcome:?}");
    };
    assert_eq!(confirm.host, "mail.hoster.example");
    assert_eq!(confirm.domain, "corp.example");
    assert_eq!(confirm.reason, RedirectReason::HttpRedirect);
    assert_eq!(confirm.url, target);
    assert!(
        server.hosts_with_credentials().is_empty(),
        "nothing sent before the user confirms"
    );

    // The user confirms the host: now the login goes there.
    let mut confirmed = request("ada@corp.example", basic("secret"));
    confirmed.trusted_hosts = vec![confirm.host];
    let found = found(d.discover(&confirmed).await.unwrap());
    assert_eq!(server.hosts_with_credentials(), ["mail.hoster.example"]);
    assert_eq!(
        found.candidates[0].warnings,
        [DiscoveryWarning::ForeignHosts]
    );
}

#[tokio::test]
async fn redirect_addr_to_exchange_online_needs_no_login() {
    let server = server(&[
        (
            "corp.example",
            MockHost {
                pox: Some(MockPox::RedirectAddr("ada@tenant.example".into())),
                login: Some(MockLogin::Negotiate),
                ..MockHost::default()
            },
        ),
        (
            "autodiscover.tenant.example",
            MockHost {
                v2: Some(MockV2::Url(ONLINE_EWS.into())),
                ..MockHost::default()
            },
        ),
    ])
    .await;
    let found = found(
        discoverer(&server, false)
            .discover(&request("ada@corp.example", windows()))
            .await
            .unwrap(),
    );
    assert_eq!(found.address, "ada@tenant.example");
    assert_eq!(found.choice.engine, ExchangeEngine::Graph);
    assert_eq!(server.hosts_with_credentials(), ["corp.example"]);
}

#[tokio::test]
async fn redirect_addr_to_another_domain_asks_before_signing_in() {
    let server = server(&[
        (
            "corp.example",
            MockHost {
                pox: Some(MockPox::RedirectAddr("ada@other.example".into())),
                ..MockHost::default()
            },
        ),
        (
            "other.example",
            basic_host(MockPox::Settings(corp_settings())),
        ),
    ])
    .await;
    let outcome = discoverer(&server, false)
        .discover(&request("ada@corp.example", basic("secret")))
        .await
        .unwrap();
    let AutodiscoverOutcome::ConfirmRedirect(confirm) = outcome else {
        panic!("a confirmation, got {outcome:?}");
    };
    assert_eq!(confirm.host, "other.example");
    assert_eq!(confirm.reason, RedirectReason::RedirectAddr);
    assert!(server.hosts_with_credentials().is_empty());
}

#[tokio::test]
async fn http_method_and_srv_targets_always_ask() {
    let target = "https://ad.corp-mail.example/autodiscover/autodiscover.xml";
    let server = server(&[
        (
            "autodiscover.corp.example",
            MockHost {
                http_redirect: Some(target.into()),
                ..MockHost::default()
            },
        ),
        (
            "ad.corp-mail.example",
            basic_host(MockPox::Settings(corp_settings())),
        ),
        (
            "mail.srv.example",
            basic_host(MockPox::Settings(corp_settings())),
        ),
    ])
    .await;
    let d = discoverer(&server, false);
    let outcome = d
        .discover(&request("ada@corp.example", basic("secret")))
        .await
        .unwrap();
    let AutodiscoverOutcome::ConfirmRedirect(confirm) = outcome else {
        panic!("a confirmation, got {outcome:?}");
    };
    assert_eq!(confirm.reason, RedirectReason::HttpMethod);
    assert_eq!(confirm.host, "ad.corp-mail.example");
    let mut confirmed = request("ada@corp.example", basic("secret"));
    confirmed.trusted_hosts = vec!["AD.corp-mail.example".into()];
    let found = found(d.discover(&confirmed).await.unwrap());
    assert!(
        found.candidates[0]
            .warnings
            .contains(&DiscoveryWarning::InsecureSource),
        "found through plain HTTP"
    );

    // SRV.
    let srv_server = server_for_srv().await;
    let dns = FakeDns(vec![SrvRecord {
        priority: 0,
        weight: 0,
        port: 443,
        target: "mail.srv.example.".into(),
    }]);
    let http = ReqwestHttp::system()
        .unwrap()
        .route_all_to(srv_server.addr());
    let d = Autodiscoverer::new(Arc::new(http))
        .with_online_host(None)
        .with_dns(Arc::new(dns));
    let outcome = d
        .discover(&request("ada@corp.example", basic("secret")))
        .await
        .unwrap();
    let AutodiscoverOutcome::ConfirmRedirect(confirm) = outcome else {
        panic!("a confirmation, got {outcome:?}");
    };
    assert_eq!(confirm.reason, RedirectReason::Srv);
    assert_eq!(confirm.host, "mail.srv.example");
    assert!(srv_server.hosts_with_credentials().is_empty());
}

async fn server_for_srv() -> MockAutodiscoverServer {
    server(&[(
        "mail.srv.example",
        basic_host(MockPox::Settings(corp_settings())),
    )])
    .await
}

#[tokio::test]
async fn loops_insecure_redirects_and_bad_addresses() {
    let server = server(&[
        (
            "corp.example",
            MockHost {
                pox: Some(MockPox::HttpRedirect(
                    "https://corp.example/autodiscover/autodiscover.xml".into(),
                )),
                ..MockHost::default()
            },
        ),
        (
            "autodiscover.loop.example",
            MockHost {
                v2: Some(MockV2::Redirect("http://mail.loop.example/plain".into())),
                ..MockHost::default()
            },
        ),
    ])
    .await;
    let d = discoverer(&server, false);
    assert_eq!(
        d.discover(&request("ada@corp.example", HttpLogin::None))
            .await
            .unwrap_err(),
        AutodiscoverError::TooManyRedirects
    );
    // A v2 redirect to plain HTTP is not followed.
    let outcome = d
        .discover(&request("ada@loop.example", HttpLogin::None))
        .await
        .unwrap();
    assert!(matches!(outcome, AutodiscoverOutcome::NotFound { .. }));
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.host != "mail.loop.example")
    );
    assert_eq!(
        d.discover(&request("not-an-address", HttpLogin::None))
            .await
            .unwrap_err(),
        AutodiscoverError::InvalidAddress
    );
}

#[tokio::test]
async fn ntlm_only_servers() {
    let server = server(&[(
        "corp.example",
        MockHost {
            pox: Some(MockPox::Settings(corp_settings())),
            login: Some(MockLogin::NtlmOnly),
            ..MockHost::default()
        },
    )])
    .await;
    let negotiate = Arc::new(FakeNegotiate::default());
    let ntlm = Arc::new(FakeNegotiate::default());
    let login = HttpLogin::Windows(WindowsLogin {
        negotiate: negotiate.clone(),
        ntlm: Some(ntlm.clone()),
    });
    let mut req = request("ada@corp.example", login);
    req.windows_username = Some("CORP\\ada".into());
    let found = found(discoverer(&server, false).discover(&req).await.unwrap());
    assert!(negotiate.spns.lock().unwrap().is_empty());
    assert_eq!(ntlm.spns.lock().unwrap().len(), 1);
    assert!(matches!(&found.candidates[0].incoming.server().auth,
        AuthConfig::Windows { username: Some(u), .. } if u == "CORP\\ada"));
    let auths: Vec<String> = server
        .requests()
        .into_iter()
        .filter_map(|r| r.authorization)
        .collect();
    assert!(auths.iter().all(|a| a.starts_with("NTLM ")), "{auths:?}");
}
