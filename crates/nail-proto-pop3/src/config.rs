//! Connection configuration and credentials.

use std::fmt;
use std::time::Duration;

use nail_api::accounts::{MailProtocol, SaslMechanism, ServerEndpoint, TlsMode, default_port};

/// Which certificates are trusted for TLS.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum TlsTrust {
    /// The operating system's trust store (`rustls-platform-verifier`). The
    /// default.
    #[default]
    Platform,
    /// Only the given DER-encoded root certificates (tests, pinned
    /// self-signed servers).
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

/// How to reach a POP3 server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pop3Config {
    /// Host name or IP address; also the TLS server name.
    pub host: String,
    /// TCP port (995 implicit TLS, 110 otherwise).
    pub port: u16,
    /// Transport security. [`TlsMode::Plaintext`] is for local test servers
    /// only.
    pub tls: TlsMode,
    /// Trusted certificates.
    pub trust: TlsTrust,
    /// Timeout for connecting, the TLS handshake and each response line
    /// (so a slow but steady download never times out).
    pub timeout: Duration,
    /// The connection log every session records into (ADR 0190); off by
    /// default.
    pub tap: nail_connlog::WireTap,
}

impl Pop3Config {
    /// Default timeout.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_mins(1);

    /// Implicit TLS on port 995 with platform trust: the recommended setup.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 995,
            tls: TlsMode::Implicit,
            trust: TlsTrust::Platform,
            timeout: Self::DEFAULT_TIMEOUT,
            tap: nail_connlog::WireTap::off(),
        }
    }

    /// Records every session into `tap` (ADR 0190).
    #[must_use]
    pub fn with_tap(mut self, tap: nail_connlog::WireTap) -> Self {
        self.tap = tap;
        self
    }

    /// The connection part of an account's incoming POP3 [`ServerEndpoint`].
    /// A port of 0 means the default port for the TLS mode.
    pub fn from_endpoint(endpoint: &ServerEndpoint) -> Self {
        let port = if endpoint.port == 0 {
            default_port(MailProtocol::Pop3, endpoint.tls)
        } else {
            endpoint.port
        };
        Self {
            host: endpoint.host.clone(),
            port,
            tls: endpoint.tls,
            trust: TlsTrust::Platform,
            timeout: Self::DEFAULT_TIMEOUT,
            tap: nail_connlog::WireTap::off(),
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

    /// Sets the timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Credentials for one login. `Debug` never prints secrets.
#[derive(Clone, PartialEq, Eq)]
pub enum Pop3Credentials {
    /// User name and password.
    Password {
        /// Login name.
        username: String,
        /// Password.
        password: String,
        /// Forced method: [`SaslMechanism::Plain`] (`AUTH PLAIN`),
        /// [`SaslMechanism::Login`] (`USER`/`PASS`) or
        /// [`SaslMechanism::Apop`]. `None`: `AUTH PLAIN` when offered, else
        /// `USER`/`PASS`, else `APOP`.
        mechanism: Option<SaslMechanism>,
    },
    /// OAuth2 bearer token.
    OAuth2 {
        /// Login name (the email address).
        username: String,
        /// A valid access token.
        access_token: String,
        /// Forced mechanism; `None` picks `XOAUTH2`, else `OAUTHBEARER`.
        mechanism: Option<SaslMechanism>,
    },
}

impl Pop3Credentials {
    /// Password credentials with automatic method choice.
    pub fn password(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Password {
            username: username.into(),
            password: password.into(),
            mechanism: None,
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

    /// The login name.
    pub fn username(&self) -> &str {
        match self {
            Self::Password { username, .. } | Self::OAuth2 { username, .. } => username,
        }
    }
}

impl fmt::Debug for Pop3Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, mechanism) = match self {
            Self::Password { mechanism, .. } => ("Password", mechanism),
            Self::OAuth2 { mechanism, .. } => ("OAuth2", mechanism),
        };
        f.debug_struct(kind)
            .field("username", &self.username())
            .field("secret", &"<redacted>")
            .field("mechanism", mechanism)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::AuthConfig;

    #[test]
    fn defaults_and_endpoints() {
        let config = Pop3Config::new("pop.example.org");
        assert_eq!((config.port, config.tls), (995, TlsMode::Implicit));
        let endpoint = ServerEndpoint {
            host: "pop.example.org".into(),
            port: 0,
            tls: TlsMode::Starttls,
            auth: AuthConfig::None,
        };
        assert_eq!(Pop3Config::from_endpoint(&endpoint).port, 110);
    }

    #[test]
    fn debug_hides_secrets() {
        let text = format!(
            "{:?} {:?}",
            Pop3Credentials::password("sam", "hunter2"),
            Pop3Credentials::oauth2("sam", "ya29.x")
        );
        assert!(text.contains("sam"));
        assert!(!text.contains("hunter2") && !text.contains("ya29"));
        assert_eq!(
            format!("{:?}", TlsTrust::CustomRoots(vec![])),
            "CustomRoots(0 certificates)"
        );
    }
}
