//! Connection configuration and credentials.

use std::fmt;
use std::time::Duration;

use nail_api::accounts::{
    MailProtocol, SaslMechanism, ServerEndpoint, SmtpOptions, TlsMode, default_port,
};

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

/// How to reach an SMTP submission server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpConfig {
    /// Host name or IP address; also the TLS server name.
    pub host: String,
    /// TCP port (465 implicit TLS, 587 STARTTLS).
    pub port: u16,
    /// Transport security. [`TlsMode::Plaintext`] is for local test servers
    /// only.
    pub tls: TlsMode,
    /// Trusted certificates.
    pub trust: TlsTrust,
    /// Timeout for connecting and for each command. The message transfer
    /// gets this plus one second per 16 KiB.
    pub timeout: Duration,
    /// The `EHLO` name. `None` sends the address literal `[127.0.0.1]`
    /// rather than the machine's host name, which would leak into the
    /// `Received` header of every sent message.
    pub hello_name: Option<String>,
}

impl SmtpConfig {
    /// Default per-operation timeout.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

    /// Implicit TLS on port 465 with platform trust: the recommended setup
    /// (RFC 8314).
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 465,
            tls: TlsMode::Implicit,
            trust: TlsTrust::Platform,
            timeout: Self::DEFAULT_TIMEOUT,
            hello_name: None,
        }
    }

    /// The connection part of an account's outgoing [`ServerEndpoint`]
    /// (credentials are resolved separately, see
    /// [`crate::SmtpAuth::from_config`]). A port of 0 means the default
    /// port for the TLS mode.
    pub fn from_endpoint(endpoint: &ServerEndpoint) -> Self {
        let port = if endpoint.port == 0 {
            default_port(MailProtocol::Smtp, endpoint.tls)
        } else {
            endpoint.port
        };
        Self {
            host: endpoint.host.clone(),
            port,
            tls: endpoint.tls,
            trust: TlsTrust::Platform,
            timeout: Self::DEFAULT_TIMEOUT,
            hello_name: None,
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

    /// Sets the `EHLO` name.
    #[must_use]
    pub fn with_hello_name(mut self, name: impl Into<String>) -> Self {
        self.hello_name = Some(name.into());
        self
    }
}

/// Credentials for one login. `Debug` never prints secrets.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum SmtpCredentials {
    /// No authentication (a relay that trusts the network).
    #[default]
    None,
    /// User name and password: `PLAIN` or `LOGIN`.
    Password {
        /// Login name.
        username: String,
        /// Password or app password.
        password: String,
        /// Forced mechanism; `None` picks `PLAIN`, else `LOGIN`.
        mechanism: Option<SaslMechanism>,
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

impl SmtpCredentials {
    /// Password credentials with automatic mechanism choice.
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
}

impl fmt::Debug for SmtpCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Password {
                username,
                mechanism,
                ..
            } => f
                .debug_struct("Password")
                .field("username", username)
                .field("password", &"<redacted>")
                .field("mechanism", mechanism)
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

/// Per-message options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SendOptions {
    /// Ask for delivery status notifications on failure and delay (RFC 3461
    /// `NOTIFY=FAILURE,DELAY`, `RET=HDRS`) when the server offers `DSN`.
    pub dsn: bool,
}

impl From<&SmtpOptions> for SendOptions {
    fn from(options: &SmtpOptions) -> Self {
        Self {
            dsn: options.dsn.unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::AuthConfig;

    #[test]
    fn defaults_are_secure() {
        let config = SmtpConfig::new("smtp.example.org");
        assert_eq!((config.port, config.tls), (465, TlsMode::Implicit));
        assert_eq!(config.trust, TlsTrust::Platform);
        assert_eq!(config.hello_name, None);
    }

    #[test]
    fn from_endpoint_fills_default_port() {
        let endpoint = ServerEndpoint {
            host: "smtp.example.org".into(),
            port: 0,
            tls: TlsMode::Starttls,
            auth: AuthConfig::None,
        };
        let config = SmtpConfig::from_endpoint(&endpoint);
        assert_eq!((config.port, config.tls), (587, TlsMode::Starttls));
        let explicit = SmtpConfig::from_endpoint(&ServerEndpoint {
            port: 2525,
            ..endpoint
        });
        assert_eq!(explicit.port, 2525);
    }

    #[test]
    fn debug_hides_secrets() {
        let text = format!(
            "{:?} {:?}",
            SmtpCredentials::password("sam", "hunter2"),
            SmtpCredentials::oauth2("sam", "ya29.token")
        );
        assert!(text.contains("sam"));
        assert!(!text.contains("hunter2"));
        assert!(!text.contains("ya29"));
        assert_eq!(
            format!("{:?}", TlsTrust::CustomRoots(vec![vec![1]])),
            "CustomRoots(1 certificates)"
        );
        assert!(SendOptions::from(&SmtpOptions { dsn: Some(true) }).dsn);
        assert!(!SendOptions::from(&SmtpOptions::default()).dsn);
    }
}
