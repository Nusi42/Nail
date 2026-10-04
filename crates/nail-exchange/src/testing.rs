//! A mock Autodiscover server for tests and the demo-multi seed (feature
//! `testing`; plan M3c WP93, contract ADR 0072).
//!
//! It binds 127.0.0.1 only (ADR 0057) and plays several hosts at once: a
//! request's `Host` header picks the [`MockHost`] that answers it (use
//! [`crate::http::ReqwestHttp::route_all_to`] so the client keeps real
//! host names). Per host it answers
//!
//! * Autodiscover v2 (`GET /autodiscover/autodiscover.json/v1.0/<address>`)
//!   with an EWS URL, a redirect or an error;
//! * POX (`POST /autodiscover/autodiscover.xml`) with settings (EWS URLs,
//!   IMAP / SMTP, display name, automapped mailboxes), `redirectAddr`,
//!   `redirectUrl` or an HTTP redirect — behind an optional login: Basic,
//!   or a scripted Negotiate / NTLM handshake ([`NEGOTIATE_TOKEN`] →
//!   [`CHALLENGE_TOKEN`] → [`AUTHENTICATE_TOKEN`]) that fakes of
//!   `NegotiateAuth` can follow;
//! * the HTTP redirect method (`GET /autodiscover/autodiscover.xml` → a
//!   redirect).
//!
//! Every request is logged ([`MockAutodiscoverServer::requests`]), with its
//! `Authorization` header, so tests can prove where credentials went.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State as AxumState;
use axum::http::{HeaderMap, Method, Response, StatusCode, Uri};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The first token a fake Negotiate client sends.
pub const NEGOTIATE_TOKEN: &[u8] = b"nail-negotiate";
/// The mock's challenge.
pub const CHALLENGE_TOKEN: &[u8] = b"nail-challenge";
/// The token that completes the fake handshake.
pub const AUTHENTICATE_TOKEN: &[u8] = b"nail-authenticate";

/// An Autodiscover v2 answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockV2 {
    /// `{"Protocol": …, "Url": …}` for every address and protocol.
    Url(String),
    /// A `302` to this location.
    Redirect(String),
    /// `{"ErrorCode": …}` with `404`.
    Error(String),
}

/// POX settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MockPoxSettings {
    /// `User/DisplayName`.
    pub display_name: Option<String>,
    /// `EXCH` `EwsUrl`.
    pub internal_ews_url: Option<String>,
    /// `EXPR` `EwsUrl`.
    pub external_ews_url: Option<String>,
    /// An `IMAP` protocol (host, port, SSL).
    pub imap: Option<(String, u16)>,
    /// An `SMTP` protocol (host, port, STARTTLS).
    pub smtp: Option<(String, u16)>,
    /// `AlternativeMailbox`es: (type, display name, address).
    pub alternative_mailboxes: Vec<(String, String, String)>,
}

/// A POX answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockPox {
    /// Settings for every address.
    Settings(MockPoxSettings),
    /// `redirectAddr`.
    RedirectAddr(String),
    /// `redirectUrl`.
    RedirectUrl(String),
    /// An HTTP `302` to this location.
    HttpRedirect(String),
}

/// The login a host wants before answering POX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockLogin {
    /// Basic with these credentials.
    Basic {
        /// User name.
        username: String,
        /// Password.
        password: String,
    },
    /// The scripted Negotiate handshake (`WWW-Authenticate: Negotiate` and
    /// `NTLM` offered).
    Negotiate,
    /// The scripted handshake under the `NTLM` scheme only.
    NtlmOnly,
}

/// What one host answers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MockHost {
    /// Autodiscover v2.
    pub v2: Option<MockV2>,
    /// POX.
    pub pox: Option<MockPox>,
    /// The login POX needs.
    pub login: Option<MockLogin>,
    /// The HTTP redirect method's target.
    pub http_redirect: Option<String>,
}

/// One logged request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockRequest {
    /// The `Host` header (without port).
    pub host: String,
    /// `GET` / `POST`.
    pub method: String,
    /// Path and query.
    pub path: String,
    /// The `Authorization` header, if any.
    pub authorization: Option<String>,
}

#[derive(Debug, Default)]
struct State {
    hosts: BTreeMap<String, MockHost>,
    any_host: Option<MockHost>,
    requests: Vec<MockRequest>,
}

type Shared = Arc<Mutex<State>>;

/// A running mock Autodiscover server; stops when dropped.
#[derive(Debug)]
pub struct MockAutodiscoverServer {
    addr: SocketAddr,
    state: Shared,
    task: JoinHandle<()>,
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The POX XML of settings.
pub fn pox_settings_xml(settings: &MockPoxSettings) -> String {
    let mut account = String::from("<AccountType>email</AccountType><Action>settings</Action>");
    if let Some(url) = &settings.internal_ews_url {
        let _ = write!(
            account,
            "<Protocol><Type>EXCH</Type><EwsUrl>{}</EwsUrl></Protocol>",
            escape(url)
        );
    }
    if let Some(url) = &settings.external_ews_url {
        let _ = write!(
            account,
            "<Protocol><Type>EXPR</Type><EwsUrl>{}</EwsUrl></Protocol>",
            escape(url)
        );
    }
    if let Some((host, port)) = &settings.imap {
        let _ = write!(
            account,
            "<Protocol><Type>IMAP</Type><Server>{}</Server><Port>{port}</Port><SSL>on</SSL></Protocol>",
            escape(host)
        );
    }
    if let Some((host, port)) = &settings.smtp {
        let _ = write!(
            account,
            "<Protocol><Type>SMTP</Type><Server>{}</Server><Port>{port}</Port><Encryption>TLS</Encryption></Protocol>",
            escape(host)
        );
    }
    for (kind, name, address) in &settings.alternative_mailboxes {
        let _ = write!(
            account,
            "<AlternativeMailbox><Type>{}</Type><DisplayName>{}</DisplayName><SmtpAddress>{}</SmtpAddress></AlternativeMailbox>",
            escape(kind),
            escape(name),
            escape(address)
        );
    }
    let user = settings
        .display_name
        .as_deref()
        .map(|n| format!("<User><DisplayName>{}</DisplayName></User>", escape(n)))
        .unwrap_or_default();
    pox_document(&format!("{user}<Account>{account}</Account>"))
}

fn pox_document(response: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <Autodiscover xmlns=\"http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006\">\
         <Response xmlns=\"{}\">{response}</Response></Autodiscover>",
        crate::autodiscover::pox::RESPONSE_SCHEMA
    )
}

struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: String,
}

impl Reply {
    fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

fn token(authorization: Option<&str>, scheme: &str) -> Option<Vec<u8>> {
    let (s, value) = authorization?.split_once(' ')?;
    if !s.eq_ignore_ascii_case(scheme) {
        return None;
    }
    BASE64.decode(value.trim()).ok()
}

/// Checks the login; `None` = signed in, else the `401` to send.
fn check_login(login: &MockLogin, authorization: Option<&str>) -> Option<Reply> {
    match login {
        MockLogin::Basic { username, password } => {
            let expected = BASE64.encode(format!("{username}:{password}"));
            match authorization.and_then(|a| a.strip_prefix("Basic ")) {
                Some(given) if given.trim() == expected => None,
                _ => Some(Reply::new(401, "").header("WWW-Authenticate", "Basic realm=\"mock\"")),
            }
        }
        MockLogin::Negotiate | MockLogin::NtlmOnly => {
            let scheme = if *login == MockLogin::NtlmOnly {
                "NTLM"
            } else {
                "Negotiate"
            };
            let offer = |reply: Reply| {
                let reply = if *login == MockLogin::Negotiate {
                    reply.header("WWW-Authenticate", "Negotiate")
                } else {
                    reply
                };
                reply.header("WWW-Authenticate", "NTLM")
            };
            match token(authorization, scheme).as_deref() {
                Some(t) if t == NEGOTIATE_TOKEN => Some(Reply::new(401, "").header(
                    "WWW-Authenticate",
                    format!("{scheme} {}", BASE64.encode(CHALLENGE_TOKEN)),
                )),
                Some(t) if t == AUTHENTICATE_TOKEN => None,
                None | Some(_) => Some(offer(Reply::new(401, ""))),
            }
        }
    }
}

fn email_in(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    text.split_once("<EMailAddress>")
        .and_then(|(_, rest)| rest.split_once("</EMailAddress>"))
        .map(|(address, _)| address.trim().to_owned())
        .unwrap_or_default()
}

fn dispatch(state: &mut State, method: &str, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Reply {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let authorization = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    state.requests.push(MockRequest {
        host: host.clone(),
        method: method.to_owned(),
        path: uri
            .path_and_query()
            .map_or_else(|| uri.path().to_owned(), |p| p.as_str().to_owned()),
        authorization: authorization.clone(),
    });
    let Some(config) = state.hosts.get(&host).or(state.any_host.as_ref()).cloned() else {
        return Reply::new(404, "");
    };
    let path = uri.path().to_ascii_lowercase();
    if method == "GET" && path.starts_with("/autodiscover/autodiscover.json/v1.0/") {
        let protocol = uri
            .query()
            .unwrap_or_default()
            .split('&')
            .find_map(|p| p.strip_prefix("Protocol="))
            .unwrap_or("EWS")
            .to_owned();
        return match config.v2 {
            Some(MockV2::Url(url)) => Reply::new(
                200,
                serde_json::json!({"Protocol": protocol, "Url": url}).to_string(),
            )
            .header("Content-Type", "application/json"),
            Some(MockV2::Redirect(location)) => Reply::new(302, "").header("Location", location),
            Some(MockV2::Error(code)) => Reply::new(
                404,
                serde_json::json!({"ErrorCode": code, "ErrorMessage": "mock"}).to_string(),
            ),
            None => Reply::new(404, ""),
        };
    }
    if path == "/autodiscover/autodiscover.xml" && method == "GET" {
        return match config.http_redirect {
            Some(location) => Reply::new(302, "").header("Location", location),
            None => Reply::new(404, ""),
        };
    }
    if path == "/autodiscover/autodiscover.xml" && method == "POST" {
        let Some(pox) = config.pox else {
            return Reply::new(404, "");
        };
        if let Some(login) = &config.login
            && let Some(denied) = check_login(login, authorization.as_deref())
        {
            return denied;
        }
        let _address = email_in(body);
        return match pox {
            MockPox::Settings(settings) => Reply::new(200, pox_settings_xml(&settings)),
            MockPox::RedirectAddr(address) => Reply::new(
                200,
                pox_document(&format!(
                    "<Account><Action>redirectAddr</Action><RedirectAddr>{}</RedirectAddr></Account>",
                    escape(&address)
                )),
            ),
            MockPox::RedirectUrl(url) => Reply::new(
                200,
                pox_document(&format!(
                    "<Account><Action>redirectUrl</Action><RedirectUrl>{}</RedirectUrl></Account>",
                    escape(&url)
                )),
            ),
            MockPox::HttpRedirect(location) => Reply::new(302, "").header("Location", location),
        };
    }
    Reply::new(404, "")
}

async fn handle(
    AxumState(state): AxumState<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let reply = {
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        dispatch(&mut state, method.as_str(), &uri, &headers, &body)
    };
    let mut response = Response::builder()
        .status(StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR));
    for (name, value) in reply.headers {
        response = response.header(name, value);
    }
    response
        .body(Body::from(reply.body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

impl MockAutodiscoverServer {
    /// Starts a server on a free loopback port that answers Autodiscover v2
    /// for every host and address with `ews_url`.
    pub async fn start(ews_url: &str) -> std::io::Result<Self> {
        let server = Self::start_hosts(BTreeMap::new()).await?;
        server.lock().any_host = Some(MockHost {
            v2: Some(MockV2::Url(ews_url.to_owned())),
            ..MockHost::default()
        });
        Ok(server)
    }

    /// Starts a server playing these hosts (lowercase names); other hosts
    /// answer `404`.
    pub async fn start_hosts(hosts: BTreeMap<String, MockHost>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let state: Shared = Arc::new(Mutex::new(State {
            hosts,
            any_host: None,
            requests: Vec::new(),
        }));
        let app = Router::new().fallback(handle).with_state(state.clone());
        let task = tokio::spawn(async move {
            drop(axum::serve(listener, app).await);
        });
        Ok(Self { addr, state, task })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets (or replaces) what a host answers.
    pub fn set_host(&self, host: &str, config: MockHost) {
        self.lock().hosts.insert(host.to_ascii_lowercase(), config);
    }

    /// The server's address (127.0.0.1 and its port).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The host to ask (`127.0.0.1:<port>`).
    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.addr.port())
    }

    /// The requests so far.
    pub fn requests(&self) -> Vec<MockRequest> {
        self.lock().requests.clone()
    }

    /// The hosts that received an `Authorization` header.
    pub fn hosts_with_credentials(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self
            .lock()
            .requests
            .iter()
            .filter(|r| r.authorization.is_some())
            .map(|r| r.host.clone())
            .collect();
        hosts.dedup();
        hosts
    }

    /// Forgets the request log.
    pub fn clear_requests(&self) {
        self.lock().requests.clear();
    }
}

impl Drop for MockAutodiscoverServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_script() {
        assert!(check_login(&MockLogin::Negotiate, None).is_some());
        let first = format!("Negotiate {}", BASE64.encode(NEGOTIATE_TOKEN));
        let reply = check_login(&MockLogin::Negotiate, Some(&first)).unwrap();
        assert_eq!(reply.status, 401);
        assert!(
            reply.headers[0]
                .1
                .ends_with(&BASE64.encode(CHALLENGE_TOKEN))
        );
        let last = format!("Negotiate {}", BASE64.encode(AUTHENTICATE_TOKEN));
        assert!(check_login(&MockLogin::Negotiate, Some(&last)).is_none());
        // Wrong scheme for an NTLM-only server.
        assert!(check_login(&MockLogin::NtlmOnly, Some(&last)).is_some());
        let basic = MockLogin::Basic {
            username: "ada".into(),
            password: "pw".into(),
        };
        let header = format!("Basic {}", BASE64.encode("ada:pw"));
        assert!(check_login(&basic, Some(&header)).is_none());
        assert!(check_login(&basic, Some("Basic eA==")).is_some());
    }

    #[test]
    fn pox_xml_parses() {
        let xml = pox_settings_xml(&MockPoxSettings {
            display_name: Some("Ada & co".into()),
            external_ews_url: Some("https://mail.corp.example/EWS/Exchange.asmx".into()),
            alternative_mailboxes: vec![(
                "Delegate".into(),
                "Support".into(),
                "support@corp.example".into(),
            )],
            ..MockPoxSettings::default()
        });
        let answer =
            crate::autodiscover::pox::parse_response(xml.as_bytes(), "ada@corp.example").unwrap();
        let crate::autodiscover::pox::PoxAnswer::Settings(settings) = answer else {
            panic!()
        };
        assert_eq!(settings.display_name.as_deref(), Some("Ada & co"));
        assert_eq!(settings.mailboxes.len(), 1);
        assert_eq!(email_in(b"<EMailAddress> a@b </EMailAddress>"), "a@b");
    }
}
