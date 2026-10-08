//! Connection configuration.

use std::fmt;
use std::time::Duration;

/// Transport security of an IMAP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Security {
    /// TLS from the first byte (port 993, RFC 8314). The default.
    #[default]
    ImplicitTls,
    /// Plaintext connect, then `STARTTLS` before authenticating (port 143).
    /// Fails if the server does not offer STARTTLS; never falls back to
    /// plaintext.
    StartTls,
    /// No TLS at all. Credentials travel in clear text: only for tests and
    /// explicitly configured local servers.
    InsecurePlaintext,
}

/// Which certificates are trusted for TLS (shared with the other protocol
/// clients).
pub use nail_net::tls::TlsTrust;

/// SASL mechanism / command used to authenticate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthMechanism {
    /// Chosen from the credentials and the server's capabilities: for a
    /// password `AUTHENTICATE PLAIN` when advertised (`AUTH=PLAIN`), else
    /// `LOGIN`; for an OAuth2 access token `OAUTHBEARER` when advertised,
    /// else `XOAUTH2`.
    #[default]
    Auto,
    /// The `LOGIN` command (password).
    Login,
    /// `AUTHENTICATE PLAIN` (RFC 4616; password).
    Plain,
    /// `AUTHENTICATE XOAUTH2` (Google / Microsoft; access token).
    XOAuth2,
    /// `AUTHENTICATE OAUTHBEARER` (RFC 7628; access token).
    OAuthBearer,
}

impl AuthMechanism {
    /// Whether the mechanism takes an OAuth2 access token.
    pub fn is_oauth(self) -> bool {
        matches!(self, Self::XOAuth2 | Self::OAuthBearer)
    }
}

/// The secret part of [`Credentials`].
#[derive(Clone, PartialEq, Eq)]
pub enum Secret {
    /// A password or app password (`LOGIN`, `PLAIN`).
    Password(String),
    /// An OAuth2 access token (`XOAUTH2`, `OAUTHBEARER`). Tokens expire:
    /// long-running users such as [`crate::MailboxWatcher`] fetch a fresh
    /// one through a [`crate::CredentialProvider`] for every connection.
    AccessToken(String),
}

/// Never prints the secret text.
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password(_) => f.write_str("Password(<redacted>)"),
            Self::AccessToken(_) => f.write_str("AccessToken(<redacted>)"),
        }
    }
}

impl Secret {
    /// The secret text.
    pub fn expose(&self) -> &str {
        match self {
            Self::Password(s) | Self::AccessToken(s) => s,
        }
    }
}

/// User name and secret. `Debug` never prints the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// User name (often the email address).
    pub username: String,
    /// Password or access token.
    pub secret: Secret,
}

impl Credentials {
    /// User name and password.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            secret: Secret::Password(password.into()),
        }
    }

    /// User name and OAuth2 access token (for `XOAUTH2` / `OAUTHBEARER`).
    pub fn oauth2(username: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            secret: Secret::AccessToken(access_token.into()),
        }
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.secret {
            Secret::Password(_) => "password",
            Secret::AccessToken(_) => "access token",
        };
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("secret", &format_args!("<redacted {kind}>"))
            .finish()
    }
}

/// What the client tells the server about itself with `ID` (RFC 2971), as
/// key / value pairs. `None` in [`ImapConfig::client_id`] sends nothing.
pub type ClientId = Vec<(String, String)>;

/// The default `ID`: `name` "Nail" and the crate version. Some providers
/// (e.g. 163.com) refuse access without `ID`; none needs more than this.
pub fn default_client_id() -> ClientId {
    vec![
        ("name".to_owned(), "Nail".to_owned()),
        ("version".to_owned(), env!("CARGO_PKG_VERSION").to_owned()),
    ]
}

/// How to reach and log in to an IMAP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapConfig {
    /// Host name or IP address; also the TLS server name.
    pub host: String,
    /// TCP port (usually 993 for implicit TLS, 143 otherwise).
    pub port: u16,
    /// Transport security.
    pub security: Security,
    /// Trusted certificates.
    pub tls_trust: TlsTrust,
    /// Authentication mechanism.
    pub auth: AuthMechanism,
    /// Timeout for connecting, the TLS handshake and each command. Commands
    /// that download message bodies use it as an *idle* timeout instead: they
    /// fail only when the server sends nothing for this long.
    pub timeout: Duration,
    /// How [`ImapClient::fetch_batch`](crate::ImapClient::fetch_batch)
    /// batches are sized.
    pub fetch_batch: FetchBatchLimits,
    /// Switch on `COMPRESS=DEFLATE` (RFC 4978) when the server offers it.
    /// Default `true`.
    pub compress: bool,
    /// Sent with `ID` when the server supports it; `None` sends nothing.
    pub client_id: Option<ClientId>,
    /// The connection log every connection made with this configuration
    /// records into (ADR 0190); off by default.
    pub tap: nail_connlog::WireTap,
}

/// Upper bounds of one body-fetch batch (one `UID FETCH` command). A batch
/// always holds at least one message, however large.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchBatchLimits {
    /// Maximum number of messages.
    pub max_messages: usize,
    /// Maximum sum of `RFC822.SIZE` in bytes.
    pub max_bytes: u64,
}

impl Default for FetchBatchLimits {
    fn default() -> Self {
        Self {
            max_messages: 50,
            max_bytes: 8 * 1024 * 1024,
        }
    }
}

impl ImapConfig {
    /// Implicit TLS on port 993 with platform trust: the recommended setup.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: nail_net::ports::IMAPS,
            security: Security::ImplicitTls,
            tls_trust: TlsTrust::Platform,
            auth: AuthMechanism::Auto,
            timeout: nail_net::DEFAULT_TIMEOUT,
            fetch_batch: FetchBatchLimits::default(),
            compress: true,
            client_id: Some(default_client_id()),
            tap: nail_connlog::WireTap::off(),
        }
    }

    /// Records every connection into `tap` (ADR 0190).
    #[must_use]
    pub fn with_tap(mut self, tap: nail_connlog::WireTap) -> Self {
        self.tap = tap;
        self
    }

    /// Sets the port.
    #[must_use]
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Sets the transport security.
    #[must_use]
    pub fn with_security(mut self, security: Security) -> Self {
        self.security = security;
        self
    }

    /// Sets the trusted certificates.
    #[must_use]
    pub fn with_tls_trust(mut self, trust: TlsTrust) -> Self {
        self.tls_trust = trust;
        self
    }

    /// Sets the authentication mechanism.
    #[must_use]
    pub fn with_auth(mut self, auth: AuthMechanism) -> Self {
        self.auth = auth;
        self
    }

    /// Sets the per-operation timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the body-fetch batch limits.
    #[must_use]
    pub fn with_fetch_batch(mut self, limits: FetchBatchLimits) -> Self {
        self.fetch_batch = limits;
        self
    }

    /// Enables or disables `COMPRESS=DEFLATE`.
    #[must_use]
    pub fn with_compress(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Sets what is sent with `ID` (`None`: no `ID` command).
    #[must_use]
    pub fn with_client_id(mut self, client_id: Option<ClientId>) -> Self {
        self.client_id = client_id;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_secure() {
        let config = ImapConfig::new("imap.example.org");
        assert_eq!(config.port, 993);
        assert_eq!(config.security, Security::ImplicitTls);
        assert_eq!(config.tls_trust, TlsTrust::Platform);
        assert_eq!(config.auth, AuthMechanism::Auto);
        assert!(config.compress);
        let id = config.client_id.unwrap();
        assert_eq!(id[0], ("name".to_owned(), "Nail".to_owned()));
    }

    #[test]
    fn debug_hides_secrets() {
        let text = format!("{:?}", Credentials::new("sam", "hunter2"));
        assert!(text.contains("sam"));
        assert!(!text.contains("hunter2"));
        let text = format!("{:?}", Credentials::oauth2("sam", "ya29.token"));
        assert!(text.contains("access token"));
        assert!(!text.contains("ya29"));
        assert_eq!(Credentials::oauth2("s", "t").secret.expose(), "t");
        let secret = Secret::Password("hunter2".into());
        assert_eq!(format!("{secret:?}"), "Password(<redacted>)");
        let token = Secret::AccessToken("ya29.token".into());
        assert_eq!(format!("{token:?}"), "AccessToken(<redacted>)");
        assert!(AuthMechanism::XOAuth2.is_oauth());
        assert!(!AuthMechanism::Plain.is_oauth());
        let trust = format!("{:?}", TlsTrust::CustomRoots(vec![vec![1, 2, 3]]));
        assert_eq!(trust, "CustomRoots(1 certificates)");
    }
}
