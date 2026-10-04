//! Server settings and credentials of configured accounts: from
//! `accounts.kdl` endpoints plus the keychain (passwords, OAuth2 refresh
//! tokens) to the protocol crates' configurations.

use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{ImapOptions, SaslMechanism};
use nail_api::engine::{AccessTokenProvider, BoxFuture, EngineError, SecretStore};
use nail_api::{AccountId, AuthConfig, ErrorCode, SecretRef, ServerEndpoint, TlsMode};
use nail_proto_imap::{
    AuthMechanism, CredentialProvider, Credentials, ImapConfig, ImapError, Security,
};
use nail_proto_pop3::{Pop3Config, Pop3Credentials};

/// Timeout of each network operation of the engines.
pub(crate) const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

/// Where a login's secret comes from.
#[derive(Clone)]
pub(crate) enum SecretSource {
    /// The keychain (passwords) or the token source (OAuth2), as configured.
    Stored {
        secrets: Arc<dyn SecretStore>,
        tokens: Arc<dyn AccessTokenProvider>,
    },
    /// A password given with the request (account setup, not stored).
    Password(String),
    /// An access token from a finished sign-in (account setup).
    AccessToken(String),
}

/// A resolved login.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Login {
    /// No authentication.
    None,
    /// User name and password.
    Password {
        username: String,
        password: String,
        mechanism: Option<SaslMechanism>,
    },
    /// User name and OAuth2 access token.
    OAuth2 {
        username: String,
        access_token: String,
        mechanism: Option<SaslMechanism>,
    },
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Password { username, .. } => write!(f, "Password({username}, <redacted>)"),
            Self::OAuth2 { username, .. } => write!(f, "OAuth2({username}, <redacted>)"),
        }
    }
}

/// The credential source of one endpoint of one account.
#[derive(Clone)]
pub(crate) struct EndpointCredentials {
    pub account: AccountId,
    pub auth: AuthConfig,
    pub source: SecretSource,
}

impl std::fmt::Debug for EndpointCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EndpointCredentials")
            .field("account", &self.account)
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl EndpointCredentials {
    /// The keychain reference of this endpoint's secret.
    pub(crate) fn secret_ref(&self) -> Option<SecretRef> {
        self.auth.secret_ref(&self.account)
    }

    /// The login for the next connection; `refresh` forces a new OAuth2
    /// access token.
    pub(crate) async fn login(&self, refresh: bool) -> Result<Login, EngineError> {
        match &self.auth {
            AuthConfig::None => Ok(Login::None),
            AuthConfig::Password {
                username,
                mechanism,
                ..
            } => {
                let password = match &self.source {
                    SecretSource::Password(p) => p.clone(),
                    SecretSource::AccessToken(_) => {
                        return Err(EngineError::permanent(
                            ErrorCode::InvalidRequest,
                            "a password login needs a password, not a sign-in",
                        ));
                    }
                    SecretSource::Stored { secrets, .. } => {
                        let reference = self.secret_ref().ok_or_else(|| {
                            EngineError::permanent(ErrorCode::Config, "no secret reference")
                        })?;
                        let store = Arc::clone(secrets);
                        let r = reference.clone();
                        tokio::task::spawn_blocking(move || store.get(&r))
                            .await
                            .map_err(|e| {
                                EngineError::temporary(ErrorCode::Internal, e.to_string())
                            })??
                            .ok_or_else(|| {
                                EngineError::permanent(
                                    ErrorCode::AuthFailed,
                                    format!("no password stored under {reference}"),
                                )
                            })?
                    }
                };
                Ok(Login::Password {
                    username: username.clone(),
                    password,
                    mechanism: *mechanism,
                })
            }
            AuthConfig::OAuth2 {
                username,
                provider,
                mechanism,
                ..
            } => {
                let access_token = match &self.source {
                    SecretSource::AccessToken(t) => t.clone(),
                    SecretSource::Password(_) => {
                        return Err(EngineError::permanent(
                            ErrorCode::InvalidRequest,
                            "an OAuth2 login needs a sign-in, not a password",
                        ));
                    }
                    SecretSource::Stored { tokens, .. } => {
                        let reference = self.secret_ref().ok_or_else(|| {
                            EngineError::permanent(ErrorCode::Config, "no secret reference")
                        })?;
                        tokens.access_token(provider, &reference, refresh).await?
                    }
                };
                Ok(Login::OAuth2 {
                    username: username.clone(),
                    access_token,
                    mechanism: *mechanism,
                })
            }
            AuthConfig::ClientCertificate { .. } => Err(EngineError::permanent(
                ErrorCode::Unsupported,
                "client certificates are not supported yet",
            )),
            // M3c: Windows sign-in is SSPI Negotiate over HTTP, done by the
            // EWS engine through `nail-exchange` (WP93 / WP95), never a
            // SASL login.
            AuthConfig::Windows { .. } => Err(EngineError::permanent(
                ErrorCode::Unsupported,
                "Windows sign-in only works for Exchange (EWS) accounts",
            )),
        }
    }
}

fn imap_mechanism(mechanism: Option<SaslMechanism>) -> AuthMechanism {
    match mechanism {
        Some(SaslMechanism::Plain) => AuthMechanism::Plain,
        Some(SaslMechanism::Login) => AuthMechanism::Login,
        Some(SaslMechanism::Xoauth2) => AuthMechanism::XOAuth2,
        Some(SaslMechanism::Oauthbearer) => AuthMechanism::OAuthBearer,
        _ => AuthMechanism::Auto,
    }
}

fn security(tls: TlsMode) -> Security {
    match tls {
        TlsMode::Implicit => Security::ImplicitTls,
        TlsMode::Starttls => Security::StartTls,
        TlsMode::Plaintext => Security::InsecurePlaintext,
    }
}

/// The IMAP client configuration of an endpoint.
pub(crate) fn imap_config(endpoint: &ServerEndpoint, options: &ImapOptions) -> ImapConfig {
    let mechanism = match &endpoint.auth {
        AuthConfig::Password { mechanism, .. } | AuthConfig::OAuth2 { mechanism, .. } => *mechanism,
        _ => None,
    };
    let mut config = ImapConfig::new(endpoint.host.clone())
        .with_port(endpoint.port)
        .with_security(security(endpoint.tls))
        .with_auth(imap_mechanism(mechanism))
        .with_timeout(NETWORK_TIMEOUT);
    if let Some(compress) = options.compress {
        config = config.with_compress(compress);
    }
    config
}

/// The POP3 client configuration of an endpoint.
pub(crate) fn pop3_config(endpoint: &ServerEndpoint) -> Pop3Config {
    Pop3Config::from_endpoint(endpoint).with_timeout(NETWORK_TIMEOUT)
}

/// POP3 credentials for a login.
pub(crate) fn pop3_credentials(login: Login) -> Pop3Credentials {
    match login {
        Login::None => Pop3Credentials::password("", ""),
        Login::Password {
            username,
            password,
            mechanism,
        } => Pop3Credentials::Password {
            username,
            password,
            mechanism,
        },
        Login::OAuth2 {
            username,
            access_token,
            mechanism,
        } => Pop3Credentials::OAuth2 {
            username,
            access_token,
            mechanism,
        },
    }
}

/// SMTP credentials for a login.
pub(crate) fn smtp_credentials(login: Login) -> nail_proto_smtp::SmtpCredentials {
    use nail_proto_smtp::SmtpCredentials;
    match login {
        Login::None => SmtpCredentials::None,
        Login::Password {
            username,
            password,
            mechanism,
        } => SmtpCredentials::Password {
            username,
            password,
            mechanism,
        },
        Login::OAuth2 {
            username,
            access_token,
            mechanism,
        } => SmtpCredentials::OAuth2 {
            username,
            access_token,
            mechanism,
        },
    }
}

impl CredentialProvider for EndpointCredentials {
    fn credentials(&self, refresh: bool) -> BoxFuture<'_, nail_proto_imap::Result<Credentials>> {
        // `refresh` asks for a new OAuth2 token (ignored for passwords).
        Box::pin(async move {
            match self.login(refresh).await {
                Ok(Login::Password {
                    username, password, ..
                }) => Ok(Credentials::new(username, password)),
                Ok(Login::OAuth2 {
                    username,
                    access_token,
                    ..
                }) => Ok(Credentials::oauth2(username, access_token)),
                Ok(Login::None) => Err(ImapError::AuthUnsupported("IMAP needs a login".to_owned())),
                Err(err) if err.code == ErrorCode::AuthFailed || !err.retryable => {
                    Err(ImapError::AuthFailed(err.message))
                }
                Err(err) => Err(ImapError::Io(std::io::Error::other(err.message))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::engine::MemorySecretStore;

    #[derive(Debug)]
    struct FixedToken;

    impl AccessTokenProvider for FixedToken {
        fn access_token<'a>(
            &'a self,
            _provider: &'a str,
            _secret: &'a SecretRef,
            force_refresh: bool,
        ) -> BoxFuture<'a, Result<String, EngineError>> {
            Box::pin(async move { Ok(if force_refresh { "fresh" } else { "cached" }.to_owned()) })
        }
    }

    fn stored() -> (Arc<MemorySecretStore>, SecretSource) {
        let secrets = Arc::new(MemorySecretStore::new());
        let source = SecretSource::Stored {
            secrets: secrets.clone(),
            tokens: Arc::new(FixedToken),
        };
        (secrets, source)
    }

    #[tokio::test]
    async fn passwords_come_from_the_keychain() {
        let (secrets, source) = stored();
        let creds = EndpointCredentials {
            account: "work".into(),
            auth: AuthConfig::Password {
                username: "ada".into(),
                secret: None,
                mechanism: None,
            },
            source,
        };
        let err = creds.login(false).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::AuthFailed);
        secrets
            .set(
                &SecretRef::parse("keychain:nail/work/password").unwrap(),
                "pw",
            )
            .unwrap();
        assert_eq!(
            creds.login(false).await.unwrap(),
            Login::Password {
                username: "ada".into(),
                password: "pw".into(),
                mechanism: None
            }
        );
        let imap = creds.credentials(false).await.unwrap();
        assert_eq!(imap.secret.expose(), "pw");
        assert!(!format!("{:?}", creds.login(false).await.unwrap()).contains("pw"));
    }

    #[tokio::test]
    async fn oauth_tokens_refresh_on_request() {
        let (_, source) = stored();
        let creds = EndpointCredentials {
            account: "work".into(),
            auth: AuthConfig::OAuth2 {
                username: "ada@gmail.com".into(),
                provider: "google".into(),
                secret: None,
                mechanism: None,
            },
            source,
        };
        let token = |l: Login| match l {
            Login::OAuth2 { access_token, .. } => access_token,
            other => panic!("{other:?}"),
        };
        assert_eq!(token(creds.login(false).await.unwrap()), "cached");
        assert_eq!(token(creds.login(true).await.unwrap()), "fresh");
        let given = EndpointCredentials {
            source: SecretSource::Password("x".into()),
            ..creds
        };
        assert_eq!(
            given.login(false).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn imap_config_follows_the_endpoint() {
        let endpoint = ServerEndpoint {
            host: "127.0.0.1".into(),
            port: 1143,
            tls: TlsMode::Plaintext,
            auth: AuthConfig::Password {
                username: "u".into(),
                secret: None,
                mechanism: Some(SaslMechanism::Login),
            },
        };
        let config = imap_config(&endpoint, &ImapOptions::default());
        assert_eq!(config.port, 1143);
        assert_eq!(config.security, Security::InsecurePlaintext);
        assert_eq!(config.auth, AuthMechanism::Login);
    }
}
