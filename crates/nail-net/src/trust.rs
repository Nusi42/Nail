//! Which certificates the line-based protocol clients (IMAP, POP3, SMTP,
//! ManageSieve) trust for TLS, and the builders of their connection
//! settings: one definition for every client (no TLS dependency here; the
//! rustls configuration is in `tls`).

use std::fmt;

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

/// Prints the number of custom roots, not their bytes.
impl fmt::Debug for TlsTrust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform => f.write_str("Platform"),
            Self::WebPkiRoots => f.write_str("WebPkiRoots"),
            Self::CustomRoots(roots) => write!(f, "CustomRoots({} certificates)", roots.len()),
        }
    }
}

/// The builder methods of a client's connection settings with the fields
/// `port: u16`, `tls: $tls`, `trust: TlsTrust` and `timeout: Duration`:
/// `with_port`, `with_tls`, `with_trust`, `with_timeout`.
#[macro_export]
macro_rules! endpoint_builders {
    ($ty:ident, $tls:ty) => {
        impl $ty {
            /// Sets the port.
            #[must_use]
            pub fn with_port(mut self, port: u16) -> Self {
                self.port = port;
                self
            }

            /// Sets the transport security.
            #[must_use]
            pub fn with_tls(mut self, tls: $tls) -> Self {
                self.tls = tls;
                self
            }

            /// Sets the trusted certificates.
            #[must_use]
            pub fn with_trust(mut self, trust: $crate::trust::TlsTrust) -> Self {
                self.trust = trust;
                self
            }

            /// Sets the per-operation timeout.
            #[must_use]
            pub fn with_timeout(mut self, timeout: ::std::time::Duration) -> Self {
                self.timeout = timeout;
                self
            }
        }
    };
}

/// The constructors of a SASL credentials enum with the variants
/// `Password { username, password, mechanism }` and
/// `OAuth2 { username, access_token, mechanism }`: `password()` and
/// `oauth2()` with automatic mechanism choice. With `password_mechanism:
/// none`, `Password` has no `mechanism` field (a client with one password
/// mechanism).
#[macro_export]
macro_rules! sasl_constructors {
    ($ty:ident) => {
        $crate::sasl_constructors!(@impl $ty { mechanism: None, });
    };
    ($ty:ident, password_mechanism: none) => {
        $crate::sasl_constructors!(@impl $ty {});
    };
    (@impl $ty:ident { $($password_extra:tt)* }) => {
        impl $ty {
            /// Password credentials with automatic mechanism choice.
            pub fn password(username: impl Into<String>, password: impl Into<String>) -> Self {
                Self::Password {
                    username: username.into(),
                    password: password.into(),
                    $($password_extra)*
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
    };
}

/// The `Debug` text of a login or credential source: `name { field: … }`
/// with only the given (non-secret) fields; `complete` says whether these
/// are all fields (else `..` follows).
pub fn debug_fields(
    f: &mut fmt::Formatter<'_>,
    name: &str,
    fields: &[(&str, &dyn fmt::Debug)],
    complete: bool,
) -> fmt::Result {
    let mut out = f.debug_struct(name);
    for (field, value) in fields {
        out.field(field, value);
    }
    if complete {
        out.finish()
    } else {
        out.finish_non_exhaustive()
    }
}

/// The `Debug` text of SASL credentials: `kind { username, <secret_field>:
/// "<redacted>"[, mechanism] }`.
pub fn debug_login(
    f: &mut fmt::Formatter<'_>,
    kind: &str,
    username: &dyn fmt::Debug,
    secret_field: &str,
    mechanism: Option<&dyn fmt::Debug>,
) -> fmt::Result {
    let mut fields: Vec<(&str, &dyn fmt::Debug)> =
        vec![("username", username), (secret_field, &REDACTED)];
    if let Some(mechanism) = mechanism {
        fields.push(("mechanism", mechanism));
    }
    debug_fields(f, kind, &fields, true)
}

/// The `Debug` stand-in of a secret value.
pub use nail_util::redact::REDACTED;

/// Defines `code()`, the API error class, of a line-protocol client's
/// error with the variants every such client has — `AuthFailed(_)`,
/// `AuthUnsupported(_)`, `StartTlsUnavailable`, `Unsupported(_)`,
/// `Tls(_)`, `Connect { .. }`, `Timeout(_)`, `Protocol(_)` — plus the arms
/// for its own variants (`ErrorCode` must be in scope).
#[macro_export]
macro_rules! line_client_codes {
    ($ty:ident { $($pat:pat => $code:expr),* $(,)? }) => {
        impl $ty {
            /// The API error code for this error.
            pub fn code(&self) -> ErrorCode {
                match self {
                    Self::AuthFailed(_) => ErrorCode::AuthFailed,
                    Self::AuthUnsupported(_) | Self::StartTlsUnavailable | Self::Unsupported(_) => {
                        ErrorCode::Unsupported
                    }
                    Self::Tls(_) => ErrorCode::Tls,
                    Self::Connect { .. } | Self::Timeout(_) | Self::Protocol(_) => ErrorCode::Network,
                    $($pat => $code,)*
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[expect(
        unreachable_pub,
        reason = "the macros define public methods for the client crates that use them"
    )]
    mod defined {
        use std::time::Duration;

        use super::TlsTrust;

        #[derive(Debug, PartialEq, Eq)]
        pub(super) struct Settings {
            pub(super) port: u16,
            pub(super) tls: bool,
            pub(super) trust: TlsTrust,
            pub(super) timeout: Duration,
        }

        crate::endpoint_builders!(Settings, bool);

        #[derive(Debug, PartialEq, Eq)]
        pub(super) enum Credentials {
            Password {
                username: String,
                password: String,
                mechanism: Option<u8>,
            },
            OAuth2 {
                username: String,
                access_token: String,
                mechanism: Option<u8>,
            },
        }

        crate::sasl_constructors!(Credentials);

        #[derive(Debug, PartialEq, Eq)]
        pub(super) enum OnePasswordMechanism {
            Password {
                username: String,
                password: String,
            },
            OAuth2 {
                username: String,
                access_token: String,
                mechanism: Option<u8>,
            },
        }

        crate::sasl_constructors!(OnePasswordMechanism, password_mechanism: none);
    }

    use defined::{Credentials, OnePasswordMechanism, Settings};

    struct Shown(&'static str, bool);

    impl fmt::Debug for Shown {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            debug_fields(
                f,
                self.0,
                &[("user", &"ada"), ("secret", &REDACTED)],
                self.1,
            )
        }
    }

    #[test]
    fn debug_fields_show_only_what_is_given() {
        assert_eq!(
            format!("{:?}", Shown("Login", true)),
            r#"Login { user: "ada", secret: "<redacted>" }"#
        );
        assert_eq!(
            format!("{:?}", Shown("Login", false)),
            r#"Login { user: "ada", secret: "<redacted>", .. }"#
        );
    }

    struct Login(Option<u8>);

    impl fmt::Debug for Login {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let mechanism: Option<&dyn fmt::Debug> = match &self.0 {
                Some(m) => Some(m),
                None => None,
            };
            debug_login(f, "Password", &"ada", "password", mechanism)
        }
    }

    #[test]
    fn debug_login_redacts_the_secret() {
        assert_eq!(
            format!("{:?}", Login(Some(1))),
            r#"Password { username: "ada", password: "<redacted>", mechanism: 1 }"#
        );
        assert_eq!(
            format!("{:?}", Login(None)),
            r#"Password { username: "ada", password: "<redacted>" }"#
        );
    }

    #[test]
    fn debug_hides_the_roots() {
        assert_eq!(format!("{:?}", TlsTrust::default()), "Platform");
        assert_eq!(format!("{:?}", TlsTrust::WebPkiRoots), "WebPkiRoots");
        assert_eq!(
            format!("{:?}", TlsTrust::CustomRoots(vec![vec![1]])),
            "CustomRoots(1 certificates)"
        );
    }

    #[test]
    fn builders_and_constructors() {
        let settings = Settings {
            port: 1,
            tls: false,
            trust: TlsTrust::Platform,
            timeout: Duration::ZERO,
        }
        .with_port(2)
        .with_tls(true)
        .with_trust(TlsTrust::WebPkiRoots)
        .with_timeout(Duration::from_secs(3));
        assert_eq!(
            settings,
            Settings {
                port: 2,
                tls: true,
                trust: TlsTrust::WebPkiRoots,
                timeout: Duration::from_secs(3)
            }
        );
        assert_eq!(
            Credentials::password("a", "b"),
            Credentials::Password {
                username: "a".into(),
                password: "b".into(),
                mechanism: None
            }
        );
        assert_eq!(
            Credentials::oauth2("a", "t"),
            Credentials::OAuth2 {
                username: "a".into(),
                access_token: "t".into(),
                mechanism: None
            }
        );
        assert_eq!(
            OnePasswordMechanism::password("a", "b"),
            OnePasswordMechanism::Password {
                username: "a".into(),
                password: "b".into()
            }
        );
        assert_eq!(
            OnePasswordMechanism::oauth2("a", "t"),
            OnePasswordMechanism::OAuth2 {
                username: "a".into(),
                access_token: "t".into(),
                mechanism: None
            }
        );
    }
}
