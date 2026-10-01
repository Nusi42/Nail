//! HTTP sign-in for Exchange endpoints: `WWW-Authenticate` parsing and the
//! request loop for Basic and Windows sign-in (HTTP Negotiate / NTLM).
//!
//! Windows sign-in runs the SPNEGO handshake of RFC 4559 over HTTP 401
//! answers: the request is sent without credentials; a `401` offering
//! `Negotiate` (or only `NTLM`) starts a [`nail_api::exchange::engine::NegotiateSession`] for
//! `HTTP/<host>`; every token it produces is sent as
//! `Authorization: Negotiate <base64>`, every server token of a further
//! `401` is fed back, until the server answers something else (a final
//! mutual-authentication token in that answer is checked too). The
//! session is any [`NegotiateAuth`] — SSPI in production
//! ([`crate::sspi::SspiNegotiate`]), a scripted fake in tests — so this
//! module has no platform code.
//!
//! NTLM binds the handshake to its TCP connection: callers give the loop
//! an [`ExchangeHttp`] that keeps connections alive (reqwest's pool does).

use std::fmt;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use nail_api::ErrorCode;
use nail_api::engine::EngineError;
use nail_api::exchange::engine::{NegotiateAuth, NegotiateStep, http_spn};
use nail_api::localized::LocalizedMessage;

use crate::http::{ExchangeHttp, HttpError, HttpRequest, HttpResponse};
use crate::texts;

/// Most 401 rounds of one handshake (Kerberos needs one or two, NTLM two).
pub const MAX_ROUNDS: usize = 6;

/// An authentication scheme of a `WWW-Authenticate` challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthScheme {
    /// SPNEGO (Kerberos with NTLM fallback; RFC 4559).
    Negotiate,
    /// Raw NTLM.
    Ntlm,
    /// Basic (RFC 7617; only over TLS).
    Basic,
    /// OAuth bearer tokens.
    Bearer,
    /// Anything else (`Digest`, vendor schemes); never used.
    Other,
}

impl AuthScheme {
    /// The scheme of a challenge name (case-insensitive).
    pub fn parse(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "negotiate" | "kerberos" => Self::Negotiate,
            "ntlm" => Self::Ntlm,
            "basic" => Self::Basic,
            "bearer" => Self::Bearer,
            _ => Self::Other,
        }
    }

    /// The name sent in `Authorization`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Negotiate => "Negotiate",
            Self::Ntlm => "NTLM",
            Self::Basic => "Basic",
            Self::Bearer => "Bearer",
            Self::Other => "Other",
        }
    }
}

/// One challenge of a `WWW-Authenticate` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// The scheme.
    pub scheme: AuthScheme,
    /// The scheme as the server wrote it.
    pub name: String,
    /// A base64 token (`Negotiate <token>`), decoded; `None` without one
    /// or when it is not valid base64.
    pub token: Option<Vec<u8>>,
    /// Auth parameters (`realm="…"`), names lowercase, values unquoted.
    pub params: Vec<(String, String)>,
}

impl Challenge {
    /// A parameter's value (`realm`).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Splits at commas outside double quotes.
fn split_top_level(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in value.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&value[start..]);
    out
}

fn parse_param(text: &str) -> Option<(String, String)> {
    let (name, value) = text.split_once('=')?;
    let name = name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let value = value.trim();
    let value = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => value.to_owned(),
    };
    Some((name.to_ascii_lowercase(), value))
}

/// Whether `text` is an auth-param (`name=value`) rather than a new
/// challenge (`Scheme …`).
fn is_param(text: &str) -> bool {
    match (text.find('='), text.find(char::is_whitespace)) {
        (Some(eq), Some(space)) => eq < space,
        (Some(eq), None) => !text[..eq].is_empty() && !text[eq..].trim_matches('=').is_empty(),
        (None, _) => false,
    }
}

/// Parses `WWW-Authenticate` header values (several challenges per value
/// and several values allowed).
pub fn parse_challenges<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<Challenge> {
    let mut out: Vec<Challenge> = Vec::new();
    for value in values {
        for piece in split_top_level(value) {
            let piece = piece.trim();
            if piece.is_empty() {
                continue;
            }
            if is_param(piece) {
                if let (Some(last), Some(param)) = (out.last_mut(), parse_param(piece)) {
                    last.params.push(param);
                }
                continue;
            }
            let (name, rest) = match piece.split_once(char::is_whitespace) {
                Some((name, rest)) => (name, rest.trim()),
                None => (piece, ""),
            };
            let mut challenge = Challenge {
                scheme: AuthScheme::parse(name),
                name: name.to_owned(),
                token: None,
                params: Vec::new(),
            };
            if !rest.is_empty() {
                if is_param(rest) {
                    challenge.params.extend(parse_param(rest));
                } else {
                    challenge.token = BASE64.decode(rest.trim()).ok();
                }
            }
            out.push(challenge);
        }
    }
    out
}

/// The challenges of a response's `WWW-Authenticate` headers.
pub fn challenges_of(response: &HttpResponse) -> Vec<Challenge> {
    parse_challenges(response.headers_named("WWW-Authenticate"))
}

/// The schemes offered, in order, without duplicates.
pub fn offered_schemes(challenges: &[Challenge]) -> Vec<AuthScheme> {
    let mut out = Vec::new();
    for c in challenges {
        if !out.contains(&c.scheme) {
            out.push(c.scheme);
        }
    }
    out
}

/// A password that never shows in `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

impl Password {
    /// Wraps a password.
    pub fn new(password: impl Into<String>) -> Self {
        Self(password.into())
    }

    /// The password.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(<redacted>)")
    }
}

/// Windows sign-in: the Negotiate context factory and, for servers that
/// offer only `NTLM`, one for raw NTLM (SPNEGO tokens are not accepted
/// there).
#[derive(Clone)]
pub struct WindowsLogin {
    /// Contexts for `Negotiate` challenges.
    pub negotiate: Arc<dyn NegotiateAuth>,
    /// Contexts for `NTLM`-only servers (`None` = use `negotiate`).
    pub ntlm: Option<Arc<dyn NegotiateAuth>>,
}

impl fmt::Debug for WindowsLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowsLogin")
            .field("negotiate", &self.negotiate)
            .field("ntlm", &self.ntlm)
            .finish()
    }
}

impl WindowsLogin {
    /// Sign-in with one context factory for every scheme.
    pub fn new(negotiate: Arc<dyn NegotiateAuth>) -> Self {
        Self {
            negotiate,
            ntlm: None,
        }
    }
}

/// How to answer a `401`.
#[derive(Debug, Clone, Default)]
pub enum HttpLogin {
    /// Not at all (the caller asks the user first).
    #[default]
    None,
    /// Basic over TLS.
    Basic {
        /// Login name (`DOMAIN\user`, `user@domain`).
        username: String,
        /// Password.
        password: Password,
    },
    /// Windows sign-in (Negotiate / Kerberos / NTLM).
    Windows(WindowsLogin),
}

impl HttpLogin {
    /// Whether the login would send credentials.
    pub fn is_some(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Why signing in failed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SignInError {
    /// The server wants a login and none was given.
    #[error("the server asks to sign in ({schemes:?})")]
    NeedsLogin {
        /// Offered schemes.
        schemes: Vec<AuthScheme>,
    },
    /// The server offers no scheme the login can use.
    #[error("no common sign-in method (server offers {offered:?})")]
    NoCommonScheme {
        /// Offered schemes.
        offered: Vec<AuthScheme>,
    },
    /// Basic would send the password without TLS.
    #[error("refusing to send a password without TLS")]
    InsecureBasic,
    /// The server rejected the credentials.
    #[error("the server rejected the sign-in")]
    Rejected,
    /// The handshake did not finish within [`MAX_ROUNDS`].
    #[error("the sign-in handshake did not finish")]
    TooManyRounds,
    /// The security package failed.
    #[error("Windows sign-in failed: {0}")]
    Sspi(String),
    /// No response.
    #[error("{0}")]
    Http(HttpError),
}

impl SignInError {
    /// The error class.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NeedsLogin { .. } | Self::Rejected | Self::Sspi(_) | Self::TooManyRounds => {
                ErrorCode::AuthFailed
            }
            Self::NoCommonScheme { .. } | Self::InsecureBasic => ErrorCode::Unsupported,
            Self::Http(e) if e.tls => ErrorCode::Tls,
            Self::Http(_) => ErrorCode::Network,
        }
    }

    /// The user-facing text (`exchange-signin-*`).
    pub fn localized(&self) -> LocalizedMessage {
        match self {
            Self::NeedsLogin { .. } => LocalizedMessage::new(texts::SIGNIN_NEEDS_LOGIN),
            Self::NoCommonScheme { offered } => LocalizedMessage::new(texts::SIGNIN_NO_COMMON).arg(
                "methods",
                offered
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::InsecureBasic => LocalizedMessage::new(texts::SIGNIN_INSECURE_BASIC),
            Self::Rejected => LocalizedMessage::new(texts::SIGNIN_REJECTED),
            Self::TooManyRounds | Self::Sspi(_) => LocalizedMessage::new(texts::SIGNIN_WINDOWS),
            Self::Http(_) => LocalizedMessage::for_code(self.code()),
        }
    }

    /// As an engine error (permanent apart from network failures).
    pub fn into_engine_error(self) -> EngineError {
        let code = self.code();
        let localized = self.localized();
        let error = if code == ErrorCode::Network {
            EngineError::temporary(code, self.to_string())
        } else {
            EngineError::permanent(code, self.to_string())
        };
        error.with_localized(localized)
    }
}

/// Whether a URL may carry a Basic password: `https`, or plain `http` to
/// a loopback test server.
pub fn basic_allowed(url: &url::Url) -> bool {
    url.scheme() == "https"
        || url
            .host_str()
            .is_some_and(nail_api::exchange::is_loopback_host)
}

/// The `Authorization` value of a Basic login.
pub fn basic_authorization(username: &str, password: &Password) -> String {
    format!(
        "Basic {}",
        BASE64.encode(format!("{username}:{}", password.expose()))
    )
}

/// Sends `request`; on a `401`, signs in with `login` and sends it again
/// (Basic once, Windows sign-in per the handshake). Any non-`401` answer
/// is returned as it is (redirects too: the caller checks them).
pub async fn send_signed_in(
    http: &dyn ExchangeHttp,
    request: HttpRequest,
    login: &HttpLogin,
) -> Result<HttpResponse, SignInError> {
    let first = http.send(&request).await.map_err(SignInError::Http)?;
    answer_challenge(http, request, first, login).await
}

/// Continues [`send_signed_in`] after the unauthenticated answer `first`
/// (for callers that check the host before sending credentials): a
/// non-`401` answer is returned as it is.
pub async fn answer_challenge(
    http: &dyn ExchangeHttp,
    request: HttpRequest,
    first: HttpResponse,
    login: &HttpLogin,
) -> Result<HttpResponse, SignInError> {
    if first.status != 401 {
        return Ok(first);
    }
    let challenges = challenges_of(&first);
    let offered = offered_schemes(&challenges);
    match login {
        HttpLogin::None => Err(SignInError::NeedsLogin { schemes: offered }),
        HttpLogin::Basic { username, password } => {
            if !offered.contains(&AuthScheme::Basic) {
                return Err(SignInError::NoCommonScheme { offered });
            }
            if !basic_allowed(&request.url) {
                return Err(SignInError::InsecureBasic);
            }
            let signed = request.with_authorization(basic_authorization(username, password));
            let answer = http.send(&signed).await.map_err(SignInError::Http)?;
            if answer.status == 401 {
                return Err(SignInError::Rejected);
            }
            Ok(answer)
        }
        HttpLogin::Windows(windows) => windows_handshake(http, request, windows, &offered).await,
    }
}

/// The token of the first challenge of `scheme` in a response.
fn server_token(response: &HttpResponse, scheme: AuthScheme) -> Option<Vec<u8>> {
    challenges_of(response)
        .into_iter()
        .find(|c| c.scheme == scheme)
        .and_then(|c| c.token)
}

async fn windows_handshake(
    http: &dyn ExchangeHttp,
    request: HttpRequest,
    windows: &WindowsLogin,
    offered: &[AuthScheme],
) -> Result<HttpResponse, SignInError> {
    let (scheme, auth) = if offered.contains(&AuthScheme::Negotiate) {
        (AuthScheme::Negotiate, &windows.negotiate)
    } else if offered.contains(&AuthScheme::Ntlm) {
        (
            AuthScheme::Ntlm,
            windows.ntlm.as_ref().unwrap_or(&windows.negotiate),
        )
    } else {
        return Err(SignInError::NoCommonScheme {
            offered: offered.to_vec(),
        });
    };
    let host = request.url.host_str().unwrap_or_default().to_owned();
    let mut session = auth
        .start(&http_spn(&host))
        .map_err(|e| SignInError::Sspi(e.message))?;
    let mut input: Option<Vec<u8>> = None;
    for _ in 0..MAX_ROUNDS {
        let step = session.step(input.as_deref()).map_err(|e| {
            if e.code == ErrorCode::AuthFailed {
                SignInError::Rejected
            } else {
                SignInError::Sspi(e.message)
            }
        })?;
        let (token, done) = match step {
            NegotiateStep::Continue(token) => (token, false),
            NegotiateStep::Done(Some(token)) => (token, true),
            NegotiateStep::Done(None) => {
                // Authenticated without a further token: nothing more to
                // send, but the server still answered 401 — rejected.
                return Err(SignInError::Rejected);
            }
        };
        let signed = request.clone().with_authorization(format!(
            "{} {}",
            scheme.as_str(),
            BASE64.encode(&token)
        ));
        let answer = http.send(&signed).await.map_err(SignInError::Http)?;
        if answer.status != 401 {
            if !done && let Some(last) = server_token(&answer, scheme) {
                // Mutual authentication: the server's last token must
                // verify (Kerberos).
                session
                    .step(Some(&last))
                    .map_err(|_| SignInError::Rejected)?;
            }
            return Ok(answer);
        }
        match server_token(&answer, scheme) {
            Some(next) if !done => input = Some(next),
            _ => return Err(SignInError::Rejected),
        }
    }
    Err(SignInError::TooManyRounds)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use nail_api::engine::BoxFuture;
    use nail_api::exchange::engine::NegotiateSession;
    use url::Url;

    use super::*;

    #[test]
    fn parses_challenges() {
        let challenges = parse_challenges([
            "Negotiate",
            "NTLM",
            r#"Basic realm="mail.corp.example", charset="UTF-8""#,
            "Negotiate YIIBhgYGKwYBBQUCoIIBejCCAXag",
        ]);
        assert_eq!(
            offered_schemes(&challenges),
            [AuthScheme::Negotiate, AuthScheme::Ntlm, AuthScheme::Basic]
        );
        assert_eq!(challenges[2].param("realm"), Some("mail.corp.example"));
        assert_eq!(challenges[2].param("charset"), Some("UTF-8"));
        assert!(challenges[0].token.is_none());
        assert_eq!(
            challenges[3].token.as_deref(),
            Some(
                BASE64
                    .decode("YIIBhgYGKwYBBQUCoIIBejCCAXag")
                    .unwrap()
                    .as_slice()
            )
        );
        let combined = parse_challenges([r#"Bearer authorization_uri="https://x, y", Negotiate"#]);
        assert_eq!(
            offered_schemes(&combined),
            [AuthScheme::Bearer, AuthScheme::Negotiate]
        );
        assert_eq!(combined[0].param("authorization_uri"), Some("https://x, y"));
        let token_with_padding = parse_challenges(["NTLM TlRMTVNTUAABAAAAB4IIog=="]);
        assert_eq!(
            &token_with_padding[0].token.as_deref().unwrap()[..7],
            b"NTLMSSP"
        );
        assert_eq!(AuthScheme::parse("Kerberos"), AuthScheme::Negotiate);
        assert_eq!(
            parse_challenges(["Digest qop=\"auth\""])[0].scheme,
            AuthScheme::Other
        );
    }

    /// An HTTP server script: answers in order, records requests.
    #[derive(Debug, Default)]
    struct Script {
        answers: Mutex<Vec<HttpResponse>>,
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl Script {
        fn new(answers: Vec<HttpResponse>) -> Self {
            Self {
                answers: Mutex::new(answers.into_iter().rev().collect()),
                seen: Mutex::default(),
            }
        }

        fn authorizations(&self) -> Vec<Option<String>> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|r| {
                    r.headers
                        .iter()
                        .find(|(n, _)| n == "Authorization")
                        .map(|(_, v)| v.clone())
                })
                .collect()
        }
    }

    impl ExchangeHttp for Script {
        fn send<'a>(
            &'a self,
            request: &'a HttpRequest,
        ) -> BoxFuture<'a, Result<HttpResponse, HttpError>> {
            self.seen.lock().unwrap().push(request.clone());
            let answer = self.answers.lock().unwrap().pop();
            Box::pin(async move { answer.ok_or_else(|| HttpError::network("no more answers")) })
        }
    }

    /// A fake SSPI: `negotiate` → expects `challenge` → `authenticate`.
    #[derive(Debug, Default)]
    struct FakeSession {
        round: u8,
        final_token: Option<Vec<u8>>,
    }

    impl NegotiateSession for FakeSession {
        fn step(&mut self, server: Option<&[u8]>) -> Result<NegotiateStep, EngineError> {
            self.round += 1;
            match (self.round, server) {
                (1, None) => Ok(NegotiateStep::Continue(b"negotiate".to_vec())),
                (2, Some(b"challenge")) => Ok(NegotiateStep::Continue(b"authenticate".to_vec())),
                (3, Some(t)) if Some(t) == self.final_token.as_deref() => {
                    Ok(NegotiateStep::Done(None))
                }
                _ => Err(EngineError::permanent(ErrorCode::AuthFailed, "bad token")),
            }
        }
    }

    #[derive(Debug, Default)]
    struct FakeAuth {
        spns: Mutex<Vec<String>>,
        final_token: Option<Vec<u8>>,
    }

    impl NegotiateAuth for FakeAuth {
        fn start(&self, spn: &str) -> Result<Box<dyn NegotiateSession>, EngineError> {
            self.spns.lock().unwrap().push(spn.to_owned());
            Ok(Box::new(FakeSession {
                round: 0,
                final_token: self.final_token.clone(),
            }))
        }
    }

    fn url() -> Url {
        Url::parse("https://mail.corp.example/autodiscover/autodiscover.xml").unwrap()
    }

    fn b64(bytes: &[u8]) -> String {
        BASE64.encode(bytes)
    }

    #[tokio::test]
    async fn negotiate_handshake_over_401s() {
        let script = Script::new(vec![
            HttpResponse::new(401, "")
                .header("WWW-Authenticate", "Negotiate")
                .header("WWW-Authenticate", "NTLM"),
            HttpResponse::new(401, "").header(
                "WWW-Authenticate",
                format!("Negotiate {}", b64(b"challenge")),
            ),
            HttpResponse::new(200, "ok")
                .header("WWW-Authenticate", format!("Negotiate {}", b64(b"mutual"))),
        ]);
        let auth = Arc::new(FakeAuth {
            final_token: Some(b"mutual".to_vec()),
            ..FakeAuth::default()
        });
        let login = HttpLogin::Windows(WindowsLogin::new(auth.clone()));
        let answer = send_signed_in(&script, HttpRequest::get(url()), &login)
            .await
            .unwrap();
        assert_eq!(answer.body, b"ok");
        assert_eq!(
            script.authorizations(),
            [
                None,
                Some(format!("Negotiate {}", b64(b"negotiate"))),
                Some(format!("Negotiate {}", b64(b"authenticate")))
            ]
        );
        assert_eq!(*auth.spns.lock().unwrap(), ["HTTP/mail.corp.example"]);
    }

    #[tokio::test]
    async fn ntlm_only_servers_use_the_ntlm_context() {
        let script = Script::new(vec![
            HttpResponse::new(401, "").header("WWW-Authenticate", "NTLM"),
            HttpResponse::new(401, "")
                .header("WWW-Authenticate", format!("NTLM {}", b64(b"challenge"))),
            HttpResponse::new(200, "ok"),
        ]);
        let negotiate = Arc::new(FakeAuth::default());
        let ntlm = Arc::new(FakeAuth::default());
        let login = HttpLogin::Windows(WindowsLogin {
            negotiate: negotiate.clone(),
            ntlm: Some(ntlm.clone()),
        });
        send_signed_in(&script, HttpRequest::get(url()), &login)
            .await
            .unwrap();
        assert!(negotiate.spns.lock().unwrap().is_empty());
        assert_eq!(ntlm.spns.lock().unwrap().len(), 1);
        assert_eq!(
            script.authorizations()[1].as_deref(),
            Some(format!("NTLM {}", b64(b"negotiate")).as_str())
        );
    }

    #[tokio::test]
    async fn rejected_and_unsupported_logins() {
        // The server answers the last token with a bare 401: rejected.
        let script = Script::new(vec![
            HttpResponse::new(401, "").header("WWW-Authenticate", "Negotiate"),
            HttpResponse::new(401, "").header(
                "WWW-Authenticate",
                format!("Negotiate {}", b64(b"challenge")),
            ),
            HttpResponse::new(401, "").header("WWW-Authenticate", "Negotiate"),
        ]);
        let login = HttpLogin::Windows(WindowsLogin::new(Arc::new(FakeAuth::default())));
        let error = send_signed_in(&script, HttpRequest::get(url()), &login)
            .await
            .unwrap_err();
        assert_eq!(error, SignInError::Rejected);
        assert_eq!(error.code(), ErrorCode::AuthFailed);
        assert_eq!(error.localized().id, texts::SIGNIN_REJECTED);

        // A wrong server token: the fake SSPI refuses it.
        let script = Script::new(vec![
            HttpResponse::new(401, "").header("WWW-Authenticate", "Negotiate"),
            HttpResponse::new(401, "")
                .header("WWW-Authenticate", format!("Negotiate {}", b64(b"forged"))),
        ]);
        let error = send_signed_in(&script, HttpRequest::get(url()), &login)
            .await
            .unwrap_err();
        assert_eq!(error, SignInError::Rejected);

        // Only Basic offered to a Windows login.
        let script = Script::new(vec![
            HttpResponse::new(401, "").header("WWW-Authenticate", "Basic realm=\"x\""),
        ]);
        let error = send_signed_in(&script, HttpRequest::get(url()), &login)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            SignInError::NoCommonScheme {
                offered: vec![AuthScheme::Basic]
            }
        );
        let engine = error.into_engine_error();
        assert_eq!(engine.code, ErrorCode::Unsupported);
        assert_eq!(engine.localized.unwrap().id, texts::SIGNIN_NO_COMMON);

        // No login: the caller must ask.
        let script = Script::new(vec![
            HttpResponse::new(401, "").header("WWW-Authenticate", "NTLM"),
        ]);
        let error = send_signed_in(&script, HttpRequest::get(url()), &HttpLogin::None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            SignInError::NeedsLogin {
                schemes: vec![AuthScheme::Ntlm]
            }
        );
    }

    #[tokio::test]
    async fn basic_only_over_tls() {
        let login = HttpLogin::Basic {
            username: "CORP\\ada".into(),
            password: Password::new("pa:ss"),
        };
        let script = Script::new(vec![
            HttpResponse::new(401, "").header("WWW-Authenticate", "Basic realm=\"corp\""),
            HttpResponse::new(200, "ok"),
        ]);
        send_signed_in(&script, HttpRequest::get(url()), &login)
            .await
            .unwrap();
        assert_eq!(
            script.authorizations()[1].as_deref(),
            Some(format!("Basic {}", b64(b"CORP\\ada:pa:ss")).as_str())
        );
        let plain = Url::parse("http://mail.corp.example/x").unwrap();
        let script = Script::new(vec![
            HttpResponse::new(401, "").header("WWW-Authenticate", "Basic"),
        ]);
        assert_eq!(
            send_signed_in(&script, HttpRequest::get(plain), &login)
                .await
                .unwrap_err(),
            SignInError::InsecureBasic
        );
        assert!(basic_allowed(&Url::parse("http://127.0.0.1:9/x").unwrap()));
        let script = Script::new(vec![
            HttpResponse::new(401, "").header("WWW-Authenticate", "Basic"),
            HttpResponse::new(401, "").header("WWW-Authenticate", "Basic"),
        ]);
        assert_eq!(
            send_signed_in(&script, HttpRequest::get(url()), &login)
                .await
                .unwrap_err(),
            SignInError::Rejected
        );
        assert!(!format!("{login:?}").contains("pa:ss"));
    }

    #[tokio::test]
    async fn answers_without_401_pass_through() {
        let script = Script::new(vec![
            HttpResponse::new(302, "").header("Location", "https://elsewhere.example/"),
        ]);
        let login = HttpLogin::Windows(WindowsLogin::new(Arc::new(FakeAuth::default())));
        let answer = send_signed_in(&script, HttpRequest::get(url()), &login)
            .await
            .unwrap();
        assert_eq!(answer.status, 302);
        assert_eq!(script.authorizations(), [None]);
    }
}
