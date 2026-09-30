//! The ManageSieve client (RFC 5804): capability greeting, `STARTTLS`
//! (never falling back to plaintext), `AUTHENTICATE` with `PLAIN`,
//! `XOAUTH2` or `OAUTHBEARER` (initial response), `CAPABILITY`,
//! `LISTSCRIPTS`, `PUTSCRIPT`, `CHECKSCRIPT`, `SETACTIVE`, `GETSCRIPT`,
//! `DELETESCRIPT`, `HAVESPACE`, `LOGOUT`.
//!
//! Scripts are sent as non-synchronizing literals (`{n+}`) with CRLF line
//! ends. `NO` answers become [`SieveError`]s by response code (`QUOTA*`,
//! `TRYLATER`, `ENCRYPT-NEEDED`) and command (`PUTSCRIPT` /
//! `CHECKSCRIPT` complaints are [`SieveError::ScriptRejected`] with the
//! server's text).

use std::fmt;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use nail_api::accounts::{SaslMechanism, ServerEndpoint, TlsMode};
use nail_api::sieve::MANAGESIEVE_DEFAULT_PORT;

use crate::error::{Result, SieveError};
use crate::transport::{Conn, Token, encode_string};
use crate::writer::to_crlf;

/// Which certificates are trusted for TLS.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum TlsTrust {
    /// The operating system's trust store (the default).
    #[default]
    Platform,
    /// Only these DER root certificates (tests, pinned servers).
    CustomRoots(Vec<Vec<u8>>),
}

impl fmt::Debug for TlsTrust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform => f.write_str("Platform"),
            Self::CustomRoots(roots) => write!(f, "CustomRoots({} certificates)", roots.len()),
        }
    }
}

/// How to reach a ManageSieve server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SieveConfig {
    /// Host name or IP address; also the TLS server name.
    pub host: String,
    /// TCP port (4190).
    pub port: u16,
    /// Transport security: `starttls` (the RFC 5804 way), `implicit` (a
    /// TLS proxy), `plaintext` (local test servers only).
    pub tls: TlsMode,
    /// Trusted certificates.
    pub trust: TlsTrust,
    /// Timeout for connecting and for each response.
    pub timeout: Duration,
}

impl SieveConfig {
    /// Default per-operation timeout.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// STARTTLS on port 4190 with platform trust.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: MANAGESIEVE_DEFAULT_PORT,
            tls: TlsMode::Starttls,
            trust: TlsTrust::Platform,
            timeout: Self::DEFAULT_TIMEOUT,
        }
    }

    /// The connection part of an effective ManageSieve endpoint
    /// ([`nail_api::sieve::ManageSieveConfig::endpoint`]); port 0 means
    /// 4190.
    pub fn from_endpoint(endpoint: &ServerEndpoint) -> Self {
        Self {
            host: endpoint.host.clone(),
            port: if endpoint.port == 0 {
                MANAGESIEVE_DEFAULT_PORT
            } else {
                endpoint.port
            },
            tls: endpoint.tls,
            trust: TlsTrust::Platform,
            timeout: Self::DEFAULT_TIMEOUT,
        }
    }

    /// Sets the port.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Sets the transport security.
    #[must_use]
    pub fn with_tls(mut self, tls: TlsMode) -> Self {
        self.tls = tls;
        self
    }

    /// Sets the trusted certificates.
    #[must_use]
    pub fn with_trust(mut self, trust: TlsTrust) -> Self {
        self.trust = trust;
        self
    }

    /// Sets the per-operation timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Credentials for one login (the account's IMAP login). `Debug` never
/// prints secrets.
#[derive(Clone, PartialEq, Eq)]
pub enum SieveCredentials {
    /// User name and password: SASL `PLAIN`.
    Password {
        /// Login name.
        username: String,
        /// Password or app password.
        password: String,
    },
    /// OAuth2 bearer token: `XOAUTH2` or `OAUTHBEARER`.
    OAuth2 {
        /// Login name (the email address).
        username: String,
        /// A valid access token.
        access_token: String,
        /// Forced mechanism; `None` picks `XOAUTH2`, else `OAUTHBEARER`.
        mechanism: Option<SaslMechanism>,
    },
}

impl SieveCredentials {
    /// Password credentials.
    pub fn password(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Password {
            username: username.into(),
            password: password.into(),
        }
    }

    /// OAuth2 credentials with automatic mechanism choice.
    pub fn oauth2(username: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self::OAuth2 {
            username: username.into(),
            access_token: access_token.into(),
            mechanism: None,
        }
    }
}

impl fmt::Debug for SieveCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password { username, .. } => f
                .debug_struct("Password")
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
            Self::OAuth2 {
                username,
                mechanism,
                ..
            } => f
                .debug_struct("OAuth2")
                .field("username", username)
                .field("access_token", &"<redacted>")
                .field("mechanism", mechanism)
                .finish(),
        }
    }
}

/// The server's capabilities (RFC 5804 section 1.7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// `IMPLEMENTATION`.
    pub implementation: Option<String>,
    /// `SASL` mechanisms (uppercase).
    pub sasl: Vec<String>,
    /// `SIEVE` extensions (lowercase).
    pub extensions: Vec<String>,
    /// `STARTTLS` offered.
    pub starttls: bool,
    /// `MAXREDIRECTS`.
    pub max_redirects: Option<u32>,
    /// `NOTIFY` methods.
    pub notify: Vec<String>,
    /// `VERSION` (RFC 5804 servers say `"1.0"`; older ones say nothing and
    /// lack `CHECKSCRIPT`).
    pub version: Option<String>,
    /// `OWNER` (after login).
    pub owner: Option<String>,
}

impl Capabilities {
    /// Whether the server has a Sieve extension.
    pub fn has_extension(&self, name: &str) -> bool {
        self.extensions.iter().any(|e| e.eq_ignore_ascii_case(name))
    }

    fn parse(lines: &[Vec<Token>]) -> Result<Self> {
        let mut caps = Self::default();
        let mut manage_sieve = false;
        for line in lines {
            let (name, value) = match line.as_slice() {
                [Token::Str(name)] => (name.to_ascii_uppercase(), None),
                [Token::Str(name), Token::Str(value), ..] => {
                    (name.to_ascii_uppercase(), Some(value.clone()))
                }
                // A capability of a later extension with another shape.
                [Token::Str(name), ..] => (name.to_ascii_uppercase(), None),
                _ => {
                    return Err(SieveError::NotManageSieve(format!(
                        "unexpected greeting line {line:?}"
                    )));
                }
            };
            let words = |v: &Option<String>| -> Vec<String> {
                v.as_deref()
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect()
            };
            match name.as_str() {
                "IMPLEMENTATION" => {
                    manage_sieve = true;
                    caps.implementation = value;
                }
                "SIEVE" => {
                    manage_sieve = true;
                    caps.extensions = words(&value)
                        .into_iter()
                        .map(|e| e.to_ascii_lowercase())
                        .collect();
                }
                "SASL" => {
                    caps.sasl = words(&value)
                        .into_iter()
                        .map(|m| m.to_ascii_uppercase())
                        .collect();
                }
                "STARTTLS" => caps.starttls = true,
                "MAXREDIRECTS" => {
                    caps.max_redirects = value.and_then(|v| v.trim().parse().ok());
                }
                "NOTIFY" => caps.notify = words(&value),
                "VERSION" => caps.version = value,
                "OWNER" => caps.owner = value,
                _ => {}
            }
        }
        if manage_sieve {
            Ok(caps)
        } else {
            Err(SieveError::NotManageSieve(
                "no IMPLEMENTATION or SIEVE capability".to_owned(),
            ))
        }
    }
}

/// A script on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptInfo {
    /// Its name.
    pub name: String,
    /// Whether it is the active one.
    pub active: bool,
}

/// The final line of a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    kind: StatusKind,
    code: Option<String>,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    Ok,
    No,
    Bye,
}

fn status(tokens: &[Token]) -> Option<Status> {
    let Some(Token::Atom(word)) = tokens.first() else {
        return None;
    };
    let kind = match word.to_ascii_uppercase().as_str() {
        "OK" => StatusKind::Ok,
        "NO" => StatusKind::No,
        "BYE" => StatusKind::Bye,
        _ => return None,
    };
    let mut rest = &tokens[1..];
    let mut code = None;
    if rest.first() == Some(&Token::Open) {
        let close = rest.iter().position(|t| *t == Token::Close)?;
        if let Some(Token::Atom(name)) = rest.get(1) {
            code = Some(name.to_ascii_uppercase());
        }
        rest = &rest[close + 1..];
    }
    let text = match rest.first() {
        Some(Token::Str(s)) => s.clone(),
        _ => String::new(),
    };
    Some(Status { kind, code, text })
}

/// A logged-in (or about to log in) ManageSieve connection.
#[derive(Debug)]
pub struct SieveSession {
    conn: Option<Conn>,
    config: SieveConfig,
    capabilities: Capabilities,
    authenticated: bool,
}

impl SieveSession {
    /// Connects, reads the capability greeting and, for `starttls`,
    /// upgrades to TLS (which the server must offer).
    pub async fn connect(config: &SieveConfig) -> Result<Self> {
        let mut conn = Conn::open(config, config.tls == TlsMode::Implicit).await?;
        let (lines, st) = read_greeting(&mut conn).await?;
        if st.kind != StatusKind::Ok {
            return Err(SieveError::NotManageSieve(st.text));
        }
        let mut capabilities = Capabilities::parse(&lines)?;
        if config.tls == TlsMode::Starttls {
            if !capabilities.starttls {
                return Err(SieveError::StartTlsUnavailable);
            }
            conn.write(b"STARTTLS\r\n").await?;
            let (_, st) = read_response(&mut conn).await?;
            if st.kind != StatusKind::Ok {
                return Err(SieveError::StartTlsUnavailable);
            }
            conn = conn.starttls(config).await?;
            // RFC 5804 2.2: the server re-issues its capabilities.
            let (lines, st) = read_greeting(&mut conn).await?;
            if st.kind != StatusKind::Ok {
                return Err(SieveError::Protocol(
                    "no capabilities after STARTTLS".to_owned(),
                ));
            }
            capabilities = Capabilities::parse(&lines)?;
        }
        Ok(Self {
            conn: Some(conn),
            config: config.clone(),
            capabilities,
            authenticated: false,
        })
    }

    /// Connects and logs in; the capabilities are refreshed after the
    /// login.
    pub async fn login(config: &SieveConfig, credentials: &SieveCredentials) -> Result<Self> {
        let mut session = Self::connect(config).await?;
        session.authenticate(credentials).await?;
        Ok(session)
    }

    /// The capabilities as last seen.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Whether the connection is encrypted.
    pub fn is_tls(&self) -> bool {
        self.conn.as_ref().is_some_and(Conn::is_tls)
    }

    fn conn(&mut self) -> Result<&mut Conn> {
        self.conn
            .as_mut()
            .ok_or_else(|| SieveError::ConnectionLost(" (logged out)".to_owned()))
    }

    /// Sends a command (verb plus string arguments) and reads the response.
    async fn command(&mut self, verb: &str, args: &[&str]) -> Result<(Vec<Vec<Token>>, Status)> {
        let mut line = verb.as_bytes().to_vec();
        for a in args {
            line.push(b' ');
            encode_string(a, &mut line);
        }
        line.extend_from_slice(b"\r\n");
        let conn = self.conn()?;
        conn.write(&line).await?;
        let (lines, st) = read_response(conn).await?;
        if st.kind == StatusKind::Bye {
            self.conn = None;
            return Err(SieveError::ConnectionLost(format!(": {}", st.text)));
        }
        Ok((lines, st))
    }

    /// Logs in with SASL `PLAIN` (passwords) or `XOAUTH2` / `OAUTHBEARER`
    /// (tokens), sending the initial response with the command.
    pub async fn authenticate(&mut self, credentials: &SieveCredentials) -> Result<()> {
        if self.config.tls != TlsMode::Plaintext && !self.is_tls() {
            return Err(SieveError::EncryptionNeeded(
                "refusing to log in without TLS".to_owned(),
            ));
        }
        let offered = self.capabilities.sasl.clone();
        if offered.is_empty() && self.capabilities.starttls && !self.is_tls() {
            return Err(SieveError::EncryptionNeeded(
                "the server offers no login before STARTTLS".to_owned(),
            ));
        }
        let has = |m: &str| offered.iter().any(|o| o == m);
        let unsupported = || SieveError::AuthUnsupported(offered.join(" "));
        let (mechanism, payload, oauth) = match credentials {
            SieveCredentials::Password { username, password } => {
                if !has("PLAIN") {
                    return Err(unsupported());
                }
                ("PLAIN", format!("\0{username}\0{password}"), false)
            }
            SieveCredentials::OAuth2 {
                username,
                access_token,
                mechanism,
            } => {
                let choice = match mechanism {
                    Some(SaslMechanism::Xoauth2) if has("XOAUTH2") => "XOAUTH2",
                    Some(SaslMechanism::Oauthbearer) if has("OAUTHBEARER") => "OAUTHBEARER",
                    Some(_) => return Err(unsupported()),
                    None if has("XOAUTH2") => "XOAUTH2",
                    None if has("OAUTHBEARER") => "OAUTHBEARER",
                    None => return Err(unsupported()),
                };
                let payload = if choice == "XOAUTH2" {
                    format!("user={username}\x01auth=Bearer {access_token}\x01\x01")
                } else {
                    let user = username.replace('=', "=3D").replace(',', "=2C");
                    format!("n,a={user},\x01auth=Bearer {access_token}\x01\x01")
                };
                (choice, payload, true)
            }
        };
        let encoded = STANDARD.encode(payload.as_bytes());
        let mut line = b"AUTHENTICATE ".to_vec();
        encode_string(mechanism, &mut line);
        line.push(b' ');
        encode_string(&encoded, &mut line);
        line.extend_from_slice(b"\r\n");
        let conn = self.conn()?;
        conn.write(&line).await?;
        loop {
            let tokens = conn.read_tokens().await?;
            if let Some(st) = status(&tokens) {
                return match st.kind {
                    StatusKind::Ok => {
                        self.authenticated = true;
                        // Refresh: some servers show more after the login.
                        self.refresh_capabilities().await?;
                        Ok(())
                    }
                    StatusKind::No => Err(match st.code.as_deref() {
                        Some("ENCRYPT-NEEDED") => SieveError::EncryptionNeeded(st.text),
                        Some("TRYLATER") => SieveError::TryLater(st.text),
                        _ => SieveError::AuthFailed(st.text),
                    }),
                    StatusKind::Bye => {
                        self.conn = None;
                        Err(SieveError::ConnectionLost(format!(": {}", st.text)))
                    }
                };
            }
            // A server challenge: an OAuth error document (answered with an
            // empty response to get the final NO), anything else is
            // cancelled.
            let answer: &[u8] = if oauth { b"\"\"\r\n" } else { b"\"*\"\r\n" };
            conn.write(answer).await?;
        }
    }

    /// Sends `CAPABILITY` and stores the answer.
    pub async fn refresh_capabilities(&mut self) -> Result<&Capabilities> {
        let (lines, st) = self.command("CAPABILITY", &[]).await?;
        if st.kind != StatusKind::Ok {
            return Err(refused("CAPABILITY", st));
        }
        let caps = Capabilities::parse(&lines)?;
        // SASL mechanisms may be hidden after the login; keep the old ones.
        let sasl = std::mem::take(&mut self.capabilities.sasl);
        self.capabilities = caps;
        if self.capabilities.sasl.is_empty() {
            self.capabilities.sasl = sasl;
        }
        Ok(&self.capabilities)
    }

    /// `LISTSCRIPTS`.
    pub async fn list_scripts(&mut self) -> Result<Vec<ScriptInfo>> {
        let (lines, st) = self.command("LISTSCRIPTS", &[]).await?;
        if st.kind != StatusKind::Ok {
            return Err(refused("LISTSCRIPTS", st));
        }
        lines
            .iter()
            .map(|line| match line.as_slice() {
                [Token::Str(name)] => Ok(ScriptInfo {
                    name: name.clone(),
                    active: false,
                }),
                [Token::Str(name), Token::Atom(a)] if a.eq_ignore_ascii_case("ACTIVE") => {
                    Ok(ScriptInfo {
                        name: name.clone(),
                        active: true,
                    })
                }
                other => Err(SieveError::Protocol(format!(
                    "unexpected LISTSCRIPTS line {other:?}"
                ))),
            })
            .collect()
    }

    /// `PUTSCRIPT`: stores `script` (line ends converted to CRLF) under
    /// `name`. Returns the server's warnings (`OK (WARNINGS)`), if any.
    pub async fn put_script(&mut self, name: &str, script: &str) -> Result<Vec<String>> {
        let script = to_crlf(script);
        let (_, st) = self.command("PUTSCRIPT", &[name, &script]).await?;
        script_status("PUTSCRIPT", st)
    }

    /// `CHECKSCRIPT` (RFC 5804 servers, `VERSION` capability): validates
    /// without storing. `Ok(None)` when the server cannot check.
    pub async fn check_script(&mut self, script: &str) -> Result<Option<Vec<String>>> {
        if self.capabilities.version.is_none() {
            return Ok(None);
        }
        let script = to_crlf(script);
        let (_, st) = self.command("CHECKSCRIPT", &[&script]).await?;
        script_status("CHECKSCRIPT", st).map(Some)
    }

    /// `SETACTIVE`; an empty name deactivates every script.
    pub async fn set_active(&mut self, name: &str) -> Result<()> {
        let (_, st) = self.command("SETACTIVE", &[name]).await?;
        ok_or_refused("SETACTIVE", st)
    }

    /// `GETSCRIPT`.
    pub async fn get_script(&mut self, name: &str) -> Result<String> {
        let (lines, st) = self.command("GETSCRIPT", &[name]).await?;
        if st.kind != StatusKind::Ok {
            return Err(refused("GETSCRIPT", st));
        }
        match lines.as_slice() {
            [line] => match line.as_slice() {
                [Token::Str(script)] => Ok(script.clone()),
                _ => Err(SieveError::Protocol("unexpected GETSCRIPT data".to_owned())),
            },
            _ => Err(SieveError::Protocol("unexpected GETSCRIPT data".to_owned())),
        }
    }

    /// `DELETESCRIPT` (the active script cannot be deleted).
    pub async fn delete_script(&mut self, name: &str) -> Result<()> {
        let (_, st) = self.command("DELETESCRIPT", &[name]).await?;
        ok_or_refused("DELETESCRIPT", st)
    }

    /// `HAVESPACE`: whether a script of `size` bytes fits.
    pub async fn have_space(&mut self, name: &str, size: usize) -> Result<()> {
        let mut line = b"HAVESPACE ".to_vec();
        encode_string(name, &mut line);
        line.extend_from_slice(format!(" {size}\r\n").as_bytes());
        let conn = self.conn()?;
        conn.write(&line).await?;
        let (_, st) = read_response(conn).await?;
        ok_or_refused("HAVESPACE", st)
    }

    /// `LOGOUT` and close.
    pub async fn logout(mut self) -> Result<()> {
        let result = self.command("LOGOUT", &[]).await.map(|_| ());
        if let Some(conn) = self.conn.take() {
            conn.shutdown().await;
        }
        match result {
            // A server may answer LOGOUT with BYE.
            Err(SieveError::ConnectionLost(_)) | Ok(()) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Reads the capability greeting: string lines up to `OK`. Anything else
/// (an IMAP or POP3 greeting on the port) is not ManageSieve.
async fn read_greeting(conn: &mut Conn) -> Result<(Vec<Vec<Token>>, Status)> {
    let mut lines = Vec::new();
    loop {
        let tokens = conn.read_tokens().await?;
        if let Some(st) = status(&tokens) {
            return Ok((lines, st));
        }
        if !matches!(tokens.first(), Some(Token::Str(_))) || lines.len() > 100 {
            return Err(SieveError::NotManageSieve(format!(
                "unexpected greeting {tokens:?}"
            )));
        }
        lines.push(tokens);
    }
}

/// Reads data lines up to the status line.
async fn read_response(conn: &mut Conn) -> Result<(Vec<Vec<Token>>, Status)> {
    let mut lines = Vec::new();
    loop {
        let tokens = conn.read_tokens().await?;
        if let Some(st) = status(&tokens) {
            return Ok((lines, st));
        }
        if lines.len() > 10_000 {
            return Err(SieveError::Protocol("response too long".to_owned()));
        }
        lines.push(tokens);
    }
}

fn refused(command: &str, st: Status) -> SieveError {
    match st.code.as_deref() {
        Some(code) if code.starts_with("QUOTA") => SieveError::Quota(st.text),
        Some("TRYLATER") => SieveError::TryLater(st.text),
        Some("ENCRYPT-NEEDED") => SieveError::EncryptionNeeded(st.text),
        _ => SieveError::Refused {
            command: command.to_owned(),
            code: st.code,
            message: st.text,
        },
    }
}

fn ok_or_refused(command: &str, st: Status) -> Result<()> {
    if st.kind == StatusKind::Ok {
        Ok(())
    } else {
        Err(refused(command, st))
    }
}

/// `OK` (with warnings) or the script complaint.
fn script_status(command: &str, st: Status) -> Result<Vec<String>> {
    match st.kind {
        StatusKind::Ok if st.code.as_deref() == Some("WARNINGS") => Ok(vec![st.text]),
        StatusKind::Ok => Ok(Vec::new()),
        _ => match st.code.as_deref() {
            None => Err(SieveError::ScriptRejected(st.text)),
            _ => Err(refused(command, st)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::tokenize;

    fn line(text: &str) -> Vec<Token> {
        tokenize(text).unwrap().0
    }

    #[test]
    fn parses_capabilities() {
        let caps = Capabilities::parse(&[
            line("\"IMPLEMENTATION\" \"Dovecot Pigeonhole\""),
            line("\"SIEVE\" \"fileinto Vacation\""),
            line("\"SASL\" \"plain xoauth2\""),
            line("\"STARTTLS\""),
            line("\"MAXREDIRECTS\" \"4\""),
            line("\"NOTIFY\" \"mailto\""),
            line("\"VERSION\" \"1.0\""),
            line("\"UNAUTHENTICATE\""),
            line("\"X-FUTURE\" (\"a\")"),
        ])
        .unwrap();
        assert_eq!(caps.implementation.as_deref(), Some("Dovecot Pigeonhole"));
        assert_eq!(caps.extensions, ["fileinto", "vacation"]);
        assert!(caps.has_extension("VACATION"));
        assert_eq!(caps.sasl, ["PLAIN", "XOAUTH2"]);
        assert!(caps.starttls);
        assert_eq!(caps.max_redirects, Some(4));
        assert_eq!(caps.notify, ["mailto"]);
        assert_eq!(caps.version.as_deref(), Some("1.0"));
        assert!(matches!(
            Capabilities::parse(&[line("* OK IMAP4rev1 ready")]),
            Err(SieveError::NotManageSieve(_))
        ));
        assert!(matches!(
            Capabilities::parse(&[]),
            Err(SieveError::NotManageSieve(_))
        ));
    }

    #[test]
    fn parses_status_lines() {
        let st = status(&line("NO (QUOTA/MAXSIZE) \"too big\"")).unwrap();
        assert_eq!(st.kind, StatusKind::No);
        assert_eq!(st.code.as_deref(), Some("QUOTA/MAXSIZE"));
        assert_eq!(st.text, "too big");
        assert!(matches!(refused("PUTSCRIPT", st), SieveError::Quota(_)));
        let st = status(&line("ok")).unwrap();
        assert_eq!(
            (st.kind, st.code, st.text),
            (StatusKind::Ok, None, String::new())
        );
        let st = status(&line("OK (WARNINGS) \"line 2: deprecated\"")).unwrap();
        assert_eq!(
            script_status("PUTSCRIPT", st).unwrap(),
            ["line 2: deprecated"]
        );
        let st = status(&line("NO \"line 1: error\"")).unwrap();
        assert_eq!(
            script_status("CHECKSCRIPT", st),
            Err(SieveError::ScriptRejected("line 1: error".into()))
        );
        let st = status(&line("NO (NONEXISTENT) \"no\"")).unwrap();
        assert!(matches!(
            ok_or_refused("SETACTIVE", st),
            Err(SieveError::Refused { code: Some(c), .. }) if c == "NONEXISTENT"
        ));
        let st = status(&line("NO (TRYLATER) \"busy\"")).unwrap();
        assert!(matches!(refused("X", st), SieveError::TryLater(_)));
        assert!(status(&line("\"nail\" ACTIVE")).is_none());
        assert!(status(&line("BYE \"shutting down\"")).is_some());
    }

    #[test]
    fn config_and_debug() {
        let c = SieveConfig::new("sieve.example.org");
        assert_eq!((c.port, c.tls), (4190, TlsMode::Starttls));
        let endpoint = ServerEndpoint {
            host: "h".into(),
            port: 0,
            tls: TlsMode::Implicit,
            auth: nail_api::accounts::AuthConfig::None,
        };
        let c = SieveConfig::from_endpoint(&endpoint)
            .with_port(1)
            .with_tls(TlsMode::Plaintext)
            .with_timeout(Duration::from_secs(1))
            .with_trust(TlsTrust::CustomRoots(vec![vec![1]]));
        assert_eq!((c.port, c.tls), (1, TlsMode::Plaintext));
        assert_eq!(SieveConfig::from_endpoint(&endpoint).port, 4190);
        assert_eq!(format!("{:?}", c.trust), "CustomRoots(1 certificates)");
        let text = format!(
            "{:?} {:?}",
            SieveCredentials::password("sam", "hunter2"),
            SieveCredentials::oauth2("sam", "ya29.token")
        );
        assert!(text.contains("sam") && !text.contains("hunter2") && !text.contains("ya29"));
    }
}
