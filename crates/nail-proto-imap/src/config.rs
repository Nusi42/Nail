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

/// Which certificates are trusted for TLS.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum TlsTrust {
    /// The operating system's trust store and revocation checks
    /// (`rustls-platform-verifier`). The default.
    #[default]
    Platform,
    /// Mozilla's root store compiled into the binary (`webpki-roots`).
    WebPkiRoots,
    /// Only the given DER-encoded root certificates (tests, pinned
    /// self-signed servers).
    CustomRoots(Vec<Vec<u8>>),
}

impl fmt::Debug for TlsTrust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform => f.write_str("Platform"),
            Self::WebPkiRoots => f.write_str("WebPkiRoots"),
            Self::CustomRoots(roots) => write!(f, "CustomRoots({} certificates)", roots.len()),
        }
    }
}

/// SASL mechanism / command used to authenticate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthMechanism {
    /// `AUTHENTICATE PLAIN` when advertised (`AUTH=PLAIN`), else `LOGIN`.
    #[default]
    Auto,
    /// The `LOGIN` command.
    Login,
    /// `AUTHENTICATE PLAIN` (RFC 4616).
    Plain,
}

/// User name and password. `Debug` never prints the password.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// User name (often the email address).
    pub username: String,
    /// Password or app password.
    pub password: String,
}

impl Credentials {
    /// Creates credentials.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
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
    /// Timeout for connecting, the TLS handshake and each command.
    pub timeout: Duration,
}

impl ImapConfig {
    /// Default per-operation timeout.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

    /// Implicit TLS on port 993 with platform trust: the recommended setup.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 993,
            security: Security::ImplicitTls,
            tls_trust: TlsTrust::Platform,
            auth: AuthMechanism::Auto,
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
    }

    #[test]
    fn debug_hides_password() {
        let text = format!("{:?}", Credentials::new("sam", "hunter2"));
        assert!(text.contains("sam"));
        assert!(!text.contains("hunter2"));
        let trust = format!("{:?}", TlsTrust::CustomRoots(vec![vec![1, 2, 3]]));
        assert_eq!(trust, "CustomRoots(1 certificates)");
    }
}
