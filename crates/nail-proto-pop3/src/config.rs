//! Connection configuration and credentials.

use std::fmt;
use std::time::Duration;

use nail_api::accounts::{MailProtocol, SaslMechanism, ServerEndpoint, TlsMode, default_port};

/// Which certificates are trusted for TLS (shared with the other protocol
/// clients).
pub use nail_net::trust::TlsTrust;

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
    /// Implicit TLS on port 995 with platform trust: the recommended setup.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: nail_net::ports::POP3S,
            tls: TlsMode::Implicit,
            trust: TlsTrust::Platform,
            timeout: nail_net::DEFAULT_TIMEOUT,
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
            timeout: nail_net::DEFAULT_TIMEOUT,
            tap: nail_connlog::WireTap::off(),
        }
    }
}

nail_net::endpoint_builders!(Pop3Config, TlsMode);

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

nail_net::sasl_constructors!(Pop3Credentials);

impl Pop3Credentials {
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
        nail_net::trust::debug_login(f, kind, &self.username(), "secret", Some(mechanism))
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
