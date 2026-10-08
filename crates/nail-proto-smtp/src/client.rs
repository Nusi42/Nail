//! One SMTP session: connect (implicit TLS, STARTTLS or plaintext), read the
//! `EHLO` capabilities, authenticate and submit messages.
//!
//! Built on `lettre`'s low-level [`AsyncSmtpConnection`] (stream, TLS,
//! reply parsing, dot-stuffing). Authentication and the `MAIL` / `RCPT`
//! parameters are done here so that every SASL mechanism, including the
//! error challenge of `XOAUTH2` / `OAUTHBEARER`, and `SIZE` / `DSN` /
//! `8BITMIME` / `SMTPUTF8` are handled uniformly.

use std::fmt;
use std::future::Future;
use std::net::Ipv4Addr;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use lettre::Address;
use lettre::transport::smtp::client::{
    AsyncSmtpConnection, Certificate, CertificateStore, TlsParameters, TlsParametersBuilder,
};
use lettre::transport::smtp::commands::{Data, Ehlo, Mail, Rcpt, Rset};
use lettre::transport::smtp::extension::{
    ClientId, MailBodyParameter, MailParameter, RcptParameter,
};
use lettre::transport::smtp::response::Response;
use nail_api::accounts::{SaslMechanism, TlsMode};
use nail_api::engine::{Envelope, SendReceipt};

use crate::capabilities::Capabilities;
use crate::config::{SendOptions, SmtpConfig, SmtpCredentials, TlsTrust};
use crate::error::{Phase, Result, SmtpError, map_lettre};

/// A raw command line. `Debug` is redacted because AUTH lines carry secrets.
struct Raw(String);

impl fmt::Display for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\r\n", self.0)
    }
}

/// An open, possibly authenticated SMTP session.
pub struct SmtpSession {
    conn: AsyncSmtpConnection,
    caps: Capabilities,
    config: SmtpConfig,
    authenticated: Option<String>,
}

impl fmt::Debug for SmtpSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpSession")
            .field("host", &self.config.host)
            .field("port", &self.config.port)
            .field("tls", &self.config.tls)
            .field("authenticated", &self.authenticated)
            .finish_non_exhaustive()
    }
}

fn hello_name(config: &SmtpConfig) -> ClientId {
    match &config.hello_name {
        Some(name) => ClientId::Domain(name.clone()),
        None => ClientId::Ipv4(Ipv4Addr::LOCALHOST),
    }
}

fn tls_parameters(config: &SmtpConfig) -> Result<TlsParameters> {
    let tls_err = |e: lettre::transport::smtp::Error| SmtpError::Tls(e.to_string());
    let mut builder = TlsParametersBuilder::new(config.host.clone());
    if let TlsTrust::CustomRoots(ders) = &config.trust {
        builder = builder.certificate_store(CertificateStore::None);
        for der in ders {
            builder =
                builder.add_root_certificate(Certificate::from_der(der.clone()).map_err(tls_err)?);
        }
    }
    builder.build_rustls().map_err(tls_err)
}

/// Whether a connection-level error message looks like a TLS failure
/// (lettre reports handshake errors as connection errors).
fn looks_like_tls(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    ["certificate", "tls", "handshake", "alert", "unknownissuer"]
        .iter()
        .any(|needle| lower.contains(needle))
}

async fn timed<T>(
    config: &SmtpConfig,
    timeout: Duration,
    phase: Phase,
    fut: impl Future<Output = std::result::Result<T, lettre::transport::smtp::Error>>,
) -> Result<T> {
    match tokio::time::timeout(timeout, fut).await {
        Err(_) => Err(SmtpError::Timeout(timeout)),
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            let mapped = map_lettre(&err, phase, config);
            Err(match mapped {
                SmtpError::Connect { message, .. }
                | SmtpError::ConnectionLost(message)
                | SmtpError::Protocol(message)
                    if looks_like_tls(&message) =>
                {
                    SmtpError::Tls(message)
                }
                other => other,
            })
        }
    }
}

fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Converts lone LFs to CRLF (RFC 5321 requires CRLF) and removes one final
/// CRLF, which the end-of-data sequence `CRLF . CRLF` supplies again.
pub(crate) fn normalize_message(message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len() + message.len() / 32);
    let mut previous = 0u8;
    for &b in message {
        if b == b'\n' && previous != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        previous = b;
    }
    if out.ends_with(b"\r\n") {
        out.truncate(out.len() - 2);
    }
    out
}

fn parse_address(text: &str) -> Result<Address> {
    text.trim()
        .parse::<Address>()
        .map_err(|e| SmtpError::InvalidAddress(format!("{text:?}: {e}")))
}

/// The OAUTHBEARER (RFC 7628) initial client response.
pub(crate) fn oauthbearer_payload(user: &str, host: &str, port: u16, token: &str) -> String {
    let user = user.replace('=', "=3D").replace(',', "=2C");
    format!("n,a={user},\x01host={host}\x01port={port}\x01auth=Bearer {token}\x01\x01")
}

/// The XOAUTH2 initial client response.
pub(crate) fn xoauth2_payload(user: &str, token: &str) -> String {
    format!("user={user}\x01auth=Bearer {token}\x01\x01")
}

impl SmtpSession {
    /// Connects, negotiates TLS as configured and reads the `EHLO`
    /// capabilities. With [`TlsMode::Starttls`] a server without `STARTTLS`
    /// is an error, never a silent downgrade.
    pub async fn connect(config: &SmtpConfig) -> Result<Self> {
        let hello = hello_name(config);
        let params = match config.tls {
            TlsMode::Plaintext => None,
            TlsMode::Implicit | TlsMode::Starttls => Some(tls_parameters(config)?),
        };
        let implicit = if config.tls == TlsMode::Implicit {
            params.clone()
        } else {
            None
        };
        let mut conn = timed(
            config,
            config.timeout,
            Phase::Connect,
            AsyncSmtpConnection::connect_tokio1(
                (config.host.as_str(), config.port),
                Some(config.timeout),
                &hello,
                implicit,
                None,
            ),
        )
        .await?;
        match (config.tls, params) {
            (TlsMode::Starttls, Some(params)) => {
                if !conn.can_starttls() {
                    conn.abort().await;
                    return Err(SmtpError::StartTlsUnavailable);
                }
                timed(
                    config,
                    config.timeout,
                    Phase::Connect,
                    conn.starttls(params, &hello),
                )
                .await?;
            }
            (TlsMode::Plaintext, _) => {
                tracing::warn!(host = %config.host, "SMTP connection without TLS");
            }
            _ => {}
        }
        // lettre keeps only a few known extensions from its own EHLO; ask
        // again for the full list (SIZE, DSN, …). RFC 5321 allows a repeated
        // EHLO at any time outside a transaction.
        let reply = timed(
            config,
            config.timeout,
            Phase::Connect,
            conn.command(Ehlo::new(hello)),
        )
        .await?;
        let caps = Capabilities::from_lines(reply.message());
        tracing::debug!(host = %config.host, server = %caps.server_name, "SMTP connected");
        Ok(Self {
            conn,
            caps,
            config: config.clone(),
            authenticated: None,
        })
    }

    /// The server's `EHLO` capabilities.
    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// Whether the session is encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.conn.is_encrypted()
    }

    /// The user the session authenticated as.
    pub fn authenticated_as(&self) -> Option<&str> {
        self.authenticated.as_deref()
    }

    async fn cmd(&mut self, phase: Phase, line: String) -> Result<Response> {
        let timeout = self.config.timeout;
        timed(&self.config, timeout, phase, self.conn.command(Raw(line))).await
    }

    /// Authenticates. [`SmtpCredentials::None`] does nothing.
    pub async fn authenticate(&mut self, credentials: &SmtpCredentials) -> Result<()> {
        let username = match credentials {
            SmtpCredentials::None => return Ok(()),
            SmtpCredentials::Password {
                username,
                password,
                mechanism,
            } => {
                if choose_password_mechanism(&self.caps, *mechanism)? == SaslMechanism::Login {
                    self.auth_login(username, password).await?;
                } else {
                    let payload = b64(format!("\0{username}\0{password}").as_bytes());
                    let reply = self
                        .cmd(Phase::Auth, format!("AUTH PLAIN {payload}"))
                        .await?;
                    self.expect_auth_success(&reply).await?;
                }
                username
            }
            SmtpCredentials::OAuth2 {
                username,
                access_token,
                mechanism,
            } => {
                let mechanism = choose_oauth_mechanism(&self.caps, *mechanism)?;
                self.auth_oauth(mechanism, username, access_token).await?;
                username
            }
        };
        self.authenticated = Some(username.clone());
        Ok(())
    }

    async fn auth_login(&mut self, username: &str, password: &str) -> Result<()> {
        let reply = self.cmd(Phase::Auth, "AUTH LOGIN".to_owned()).await?;
        if reply.has_code(334) {
            let reply = self.cmd(Phase::Auth, b64(username.as_bytes())).await?;
            if reply.has_code(334) {
                let reply = self.cmd(Phase::Auth, b64(password.as_bytes())).await?;
                return self.expect_auth_success(&reply).await;
            }
            return self.expect_auth_success(&reply).await;
        }
        self.expect_auth_success(&reply).await
    }

    async fn auth_oauth(
        &mut self,
        mechanism: SaslMechanism,
        username: &str,
        token: &str,
    ) -> Result<()> {
        let (name, payload, dummy) = if mechanism == SaslMechanism::Oauthbearer {
            let payload = oauthbearer_payload(username, &self.config.host, self.config.port, token);
            ("OAUTHBEARER", payload, "AQ==")
        } else {
            ("XOAUTH2", xoauth2_payload(username, token), "")
        };
        let reply = self
            .cmd(
                Phase::Auth,
                format!("AUTH {name} {}", b64(payload.as_bytes())),
            )
            .await?;
        if reply.has_code(334) {
            // Error challenge (JSON with the HTTP-like status); the client
            // must answer with a dummy response and gets the final failure.
            let detail = reply
                .first_line()
                .and_then(|l| STANDARD.decode(l.trim()).ok())
                .map(|json| String::from_utf8_lossy(&json).into_owned())
                .unwrap_or_default();
            return match self.cmd(Phase::Auth, dummy.to_owned()).await {
                Err(SmtpError::AuthFailed(message)) => {
                    Err(SmtpError::AuthFailed(format!("{message} {detail}")))
                }
                Err(other) => Err(other),
                Ok(_) => Err(SmtpError::AuthFailed(format!("token rejected {detail}"))),
            };
        }
        self.expect_auth_success(&reply).await
    }

    async fn expect_auth_success(&mut self, reply: &Response) -> Result<()> {
        if reply.has_code(235) {
            return Ok(());
        }
        if reply.has_code(334) {
            // An unexpected challenge: cancel the exchange.
            drop(self.cmd(Phase::Auth, "*".to_owned()).await);
        }
        Err(SmtpError::Protocol(format!(
            "unexpected reply {} to AUTH",
            u16::from(reply.code())
        )))
    }

    /// Submits `message` (complete raw MIME, `Bcc` removed) to the
    /// envelope's recipients.
    ///
    /// Checks the server's `SIZE` limit before sending, declares `SIZE=`,
    /// `BODY=8BITMIME` / `SMTPUTF8` when needed and requests DSNs when
    /// `options.dsn` is set and offered. A permanently refused recipient
    /// fails the whole submission (nothing is sent), naming the address.
    pub async fn send(
        &mut self,
        envelope: &Envelope,
        message: &[u8],
        options: SendOptions,
    ) -> Result<SendReceipt> {
        if envelope.to.is_empty() {
            return Err(SmtpError::InvalidAddress("no recipients".to_owned()));
        }
        let from = if envelope.from.trim().is_empty() {
            None
        } else {
            Some(parse_address(&envelope.from)?)
        };
        let recipients = envelope
            .to
            .iter()
            .map(|to| parse_address(to))
            .collect::<Result<Vec<_>>>()?;
        let body = normalize_message(message);
        let size = body.len() + 2;
        if let Some(limit) = self.caps.size_limit()
            && size as u64 > limit
        {
            return Err(SmtpError::TooLarge(format!(
                "{size} bytes; the server accepts at most {limit}"
            )));
        }

        let mut mail_params = Vec::new();
        if self.caps.has("SIZE") {
            mail_params.push(MailParameter::Size(size));
        }
        if !body.is_ascii() {
            if !self.caps.has("8BITMIME") {
                return Err(SmtpError::Unsupported(
                    "the message has 8-bit content but the server lacks 8BITMIME".to_owned(),
                ));
            }
            mail_params.push(MailParameter::Body(MailBodyParameter::EightBitMime));
        }
        let non_ascii_address = from
            .iter()
            .chain(&recipients)
            .any(|a: &Address| !AsRef::<str>::as_ref(a).is_ascii());
        if non_ascii_address {
            if !self.caps.has("SMTPUTF8") {
                return Err(SmtpError::Unsupported(
                    "an address is internationalized but the server lacks SMTPUTF8".to_owned(),
                ));
            }
            mail_params.push(MailParameter::SmtpUtfEight);
        }
        let dsn = options.dsn && self.caps.has("DSN");
        if dsn {
            mail_params.push(MailParameter::Other {
                keyword: "RET".to_owned(),
                value: Some("HDRS".to_owned()),
            });
        }

        let timeout = self.config.timeout;
        let result = async {
            timed(
                &self.config,
                timeout,
                Phase::Envelope,
                self.conn.command(Mail::new(from, mail_params)),
            )
            .await?;
            for recipient in &recipients {
                let mut params = Vec::new();
                if dsn {
                    params.push(RcptParameter::Other {
                        keyword: "NOTIFY".to_owned(),
                        value: Some("FAILURE,DELAY".to_owned()),
                    });
                    params.push(RcptParameter::Other {
                        keyword: "ORCPT".to_owned(),
                        value: Some(format!("rfc822;{recipient}")),
                    });
                }
                timed(
                    &self.config,
                    timeout,
                    Phase::Envelope,
                    self.conn.command(Rcpt::new(recipient.clone(), params)),
                )
                .await
                .map_err(|err| match err {
                    SmtpError::Rejected { code, message } => SmtpError::Rejected {
                        code,
                        message: format!("recipient <{recipient}>: {message}"),
                    },
                    SmtpError::Transient { code, message } => SmtpError::Transient {
                        code,
                        message: format!("recipient <{recipient}>: {message}"),
                    },
                    other => other,
                })?;
            }
            let reply = timed(&self.config, timeout, Phase::Data, self.conn.command(Data)).await?;
            if !reply.has_code(354) {
                return Err(SmtpError::Protocol(format!(
                    "unexpected reply {} to DATA",
                    u16::from(reply.code())
                )));
            }
            // Slow uplinks: allow one extra second per 16 KiB.
            let transfer = timeout + Duration::from_secs((body.len() / (16 * 1024)) as u64);
            timed(
                &self.config,
                transfer,
                Phase::Data,
                self.conn.message(&body),
            )
            .await
        }
        .await;
        match result {
            Ok(reply) => {
                let text: Vec<&str> = reply.message().collect();
                Ok(SendReceipt {
                    response: format!("{} {}", u16::from(reply.code()), text.join(" ")),
                })
            }
            Err(err) => {
                // Leave the connection usable for another attempt.
                if !matches!(err, SmtpError::Timeout(_) | SmtpError::ConnectionLost(_)) {
                    drop(tokio::time::timeout(timeout, self.conn.command(Rset)).await);
                }
                Err(err)
            }
        }
    }

    /// Checks the connection with `NOOP`.
    pub async fn noop(&mut self) -> Result<()> {
        self.cmd(Phase::Other, "NOOP".to_owned()).await.map(|_| ())
    }

    /// Ends the session politely with `QUIT`.
    pub async fn quit(mut self) -> Result<()> {
        let timeout = self.config.timeout;
        let result = timed(&self.config, timeout, Phase::Other, self.conn.quit()).await;
        self.conn.abort().await;
        result.map(|_| ())
    }
}

/// The first mechanism the server `offered` of the `forced` one (it must
/// have a `name`, else it is `wrong_kind`), or of the `defaults` in order.
fn first_offered(
    offered: &[String],
    forced: Option<SaslMechanism>,
    defaults: [SaslMechanism; 2],
    name: impl Fn(SaslMechanism) -> Option<&'static str>,
    wrong_kind: &str,
) -> Result<Option<SaslMechanism>> {
    let candidates = match forced {
        Some(m) if name(m).is_some() => vec![m],
        Some(m) => return Err(SmtpError::AuthUnsupported(format!("{m:?} {wrong_kind}"))),
        None => defaults.to_vec(),
    };
    Ok(candidates
        .into_iter()
        .find(|m| name(*m).is_some_and(|n| offered.iter().any(|o| o == n))))
}

/// `PLAIN` if offered, else `LOGIN`; an explicit choice must be offered.
fn choose_password_mechanism(
    caps: &Capabilities,
    forced: Option<SaslMechanism>,
) -> Result<SaslMechanism> {
    let offered = caps.auth_mechanisms();
    if offered.is_empty() {
        return Err(SmtpError::AuthUnsupported(
            "the server offers no authentication (AUTH) on this connection".to_owned(),
        ));
    }
    let name = |m: SaslMechanism| match m {
        SaslMechanism::Plain => Some("PLAIN"),
        SaslMechanism::Login => Some("LOGIN"),
        _ => None,
    };
    let defaults = [SaslMechanism::Plain, SaslMechanism::Login];
    first_offered(
        &offered,
        forced,
        defaults,
        name,
        "is not available for SMTP password logins",
    )?
    .ok_or_else(|| SmtpError::AuthUnsupported(format!("the server offers {}", offered.join(" "))))
}

/// `XOAUTH2` if offered, else `OAUTHBEARER`; an explicit choice must be
/// offered.
fn choose_oauth_mechanism(
    caps: &Capabilities,
    forced: Option<SaslMechanism>,
) -> Result<SaslMechanism> {
    let offered = caps.auth_mechanisms();
    let name = |m: SaslMechanism| match m {
        SaslMechanism::Xoauth2 => Some("XOAUTH2"),
        SaslMechanism::Oauthbearer => Some("OAUTHBEARER"),
        _ => None,
    };
    let defaults = [SaslMechanism::Xoauth2, SaslMechanism::Oauthbearer];
    first_offered(
        &offered,
        forced,
        defaults,
        name,
        "is not an OAuth2 mechanism",
    )?
    .ok_or_else(|| {
        SmtpError::AuthUnsupported(format!(
            "no OAuth2 mechanism offered (the server offers {})",
            if offered.is_empty() {
                "none".to_owned()
            } else {
                offered.join(" ")
            }
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_line_endings() {
        assert_eq!(normalize_message(b"a\nb\r\nc\r\n"), b"a\r\nb\r\nc".to_vec());
        assert_eq!(normalize_message(b"a"), b"a".to_vec());
        assert_eq!(normalize_message(b"a\r\n\r\n"), b"a\r\n".to_vec());
    }

    #[test]
    fn sasl_payloads() {
        assert_eq!(
            xoauth2_payload("a@b", "t"),
            "user=a@b\x01auth=Bearer t\x01\x01"
        );
        assert_eq!(
            oauthbearer_payload("a,=b", "h", 587, "t"),
            "n,a=a=2C=3Db,\x01host=h\x01port=587\x01auth=Bearer t\x01\x01"
        );
    }

    #[test]
    fn mechanism_choice() {
        let caps = Capabilities::from_lines(["x", "AUTH LOGIN XOAUTH2"]);
        assert_eq!(
            choose_password_mechanism(&caps, None).unwrap(),
            SaslMechanism::Login
        );
        assert!(choose_password_mechanism(&caps, Some(SaslMechanism::Plain)).is_err());
        assert!(choose_password_mechanism(&caps, Some(SaslMechanism::ScramSha256)).is_err());
        assert_eq!(
            choose_oauth_mechanism(&caps, None).unwrap(),
            SaslMechanism::Xoauth2
        );
        assert!(choose_oauth_mechanism(&caps, Some(SaslMechanism::Oauthbearer)).is_err());
        let both = Capabilities::from_lines(["x", "AUTH PLAIN LOGIN OAUTHBEARER"]);
        assert_eq!(
            choose_password_mechanism(&both, None).unwrap(),
            SaslMechanism::Plain
        );
        assert_eq!(
            choose_oauth_mechanism(&both, None).unwrap(),
            SaslMechanism::Oauthbearer
        );
        let none = Capabilities::from_lines(["x"]);
        assert!(matches!(
            choose_password_mechanism(&none, None),
            Err(SmtpError::AuthUnsupported(_))
        ));
        assert!(matches!(
            choose_oauth_mechanism(&none, None),
            Err(SmtpError::AuthUnsupported(_))
        ));
    }

    #[test]
    fn tls_heuristic() {
        assert!(looks_like_tls("invalid peer certificate: UnknownIssuer"));
        assert!(!looks_like_tls("connection refused"));
    }
}
