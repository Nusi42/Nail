//! [`SmtpTransport`]: the [`OutgoingTransport`] of an account, and
//! [`test_login`] for account setup.

use std::fmt;
use std::sync::Arc;

use nail_api::ErrorCode;
use nail_api::accounts::{AuthConfig, EndpointTestResult, SaslMechanism, SecretRef};
use nail_api::engine::{
    AccessTokenProvider, BoxFuture, EngineError, Envelope, OutgoingTransport, SecretStore,
    SendReceipt,
};
use nail_api::ids::AccountId;

use crate::client::SmtpSession;
use crate::config::{SendOptions, SmtpConfig, SmtpCredentials};

/// Where an [`SmtpTransport`] gets its credentials from, per connection.
#[derive(Clone)]
pub enum SmtpAuth {
    /// No authentication.
    None,
    /// Fixed credentials (tests, account setup before saving).
    Credentials(SmtpCredentials),
    /// A password read from the keychain on every connection.
    StoredPassword {
        /// Login name.
        username: String,
        /// Keychain reference.
        secret: SecretRef,
        /// Secret storage.
        store: Arc<dyn SecretStore>,
        /// Forced mechanism, else automatic.
        mechanism: Option<SaslMechanism>,
    },
    /// An OAuth2 access token from the token provider; a rejected token is
    /// refreshed once and the login retried.
    OAuth2 {
        /// Login name (the email address).
        username: String,
        /// Provider id (`google`, `microsoft`, …).
        provider: String,
        /// Keychain reference of the refresh token.
        secret: SecretRef,
        /// Token source.
        tokens: Arc<dyn AccessTokenProvider>,
        /// Forced mechanism, else automatic.
        mechanism: Option<SaslMechanism>,
    },
}

impl fmt::Debug for SmtpAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Credentials(c) => f.debug_tuple("Credentials").field(c).finish(),
            Self::StoredPassword {
                username, secret, ..
            } => f
                .debug_struct("StoredPassword")
                .field("username", username)
                .field("secret", secret)
                .finish_non_exhaustive(),
            Self::OAuth2 {
                username,
                provider,
                secret,
                ..
            } => f
                .debug_struct("OAuth2")
                .field("username", username)
                .field("provider", provider)
                .field("secret", secret)
                .finish_non_exhaustive(),
        }
    }
}

impl SmtpAuth {
    /// The credential source for an account's outgoing [`AuthConfig`]
    /// (`accounts.kdl`): passwords from `store`, OAuth2 tokens from
    /// `tokens`, default keychain references filled in for `account`.
    pub fn from_config(
        account: &AccountId,
        auth: &AuthConfig,
        store: Arc<dyn SecretStore>,
        tokens: Arc<dyn AccessTokenProvider>,
    ) -> Result<Self, EngineError> {
        let secret = auth.secret_ref(account);
        Ok(match (auth, secret) {
            (AuthConfig::None, _) => Self::None,
            (
                AuthConfig::Password {
                    username,
                    mechanism,
                    ..
                },
                Some(secret),
            ) => Self::StoredPassword {
                username: username.clone(),
                secret,
                store,
                mechanism: *mechanism,
            },
            (
                AuthConfig::OAuth2 {
                    username,
                    provider,
                    mechanism,
                    ..
                },
                Some(secret),
            ) => Self::OAuth2 {
                username: username.clone(),
                provider: provider.clone(),
                secret,
                tokens,
                mechanism: *mechanism,
            },
            _ => {
                return Err(EngineError::permanent(
                    ErrorCode::Unsupported,
                    "client certificates are not supported for SMTP yet",
                ));
            }
        })
    }

    fn is_oauth(&self) -> bool {
        matches!(self, Self::OAuth2 { .. })
    }

    /// The credentials for one login.
    pub async fn credentials(&self, force_refresh: bool) -> Result<SmtpCredentials, EngineError> {
        match self {
            Self::None => Ok(SmtpCredentials::None),
            Self::Credentials(c) => Ok(c.clone()),
            Self::StoredPassword {
                username,
                secret,
                store,
                mechanism,
            } => {
                let store = Arc::clone(store);
                let secret = secret.clone();
                let reference = secret.to_string();
                let password = tokio::task::spawn_blocking(move || store.get(&secret))
                    .await
                    .map_err(|e| EngineError::temporary(ErrorCode::Internal, e.to_string()))??
                    .ok_or_else(|| {
                        EngineError::permanent(
                            ErrorCode::AuthFailed,
                            format!("no password stored under {reference}"),
                        )
                    })?;
                Ok(SmtpCredentials::Password {
                    username: username.clone(),
                    password,
                    mechanism: *mechanism,
                })
            }
            Self::OAuth2 {
                username,
                provider,
                secret,
                tokens,
                mechanism,
            } => {
                let access_token = tokens.access_token(provider, secret, force_refresh).await?;
                Ok(SmtpCredentials::OAuth2 {
                    username: username.clone(),
                    access_token,
                    mechanism: *mechanism,
                })
            }
        }
    }
}

/// Sends through one account's SMTP server. Each message uses a fresh
/// connection (submission volume is low, and a fresh login picks up changed
/// passwords and tokens).
#[derive(Debug, Clone)]
pub struct SmtpTransport {
    config: SmtpConfig,
    auth: SmtpAuth,
    options: SendOptions,
}

impl SmtpTransport {
    /// A transport for `config`, logging in with `auth`.
    pub fn new(config: SmtpConfig, auth: SmtpAuth, options: SendOptions) -> Self {
        Self {
            config,
            auth,
            options,
        }
    }

    /// The server configuration.
    pub fn config(&self) -> &SmtpConfig {
        &self.config
    }

    async fn attempt(
        &self,
        envelope: &Envelope,
        message: &[u8],
        force_refresh: bool,
    ) -> Result<SendReceipt, EngineError> {
        let credentials = self.auth.credentials(force_refresh).await?;
        let mut session = SmtpSession::connect(&self.config).await?;
        session.authenticate(&credentials).await?;
        let receipt = session.send(envelope, message, self.options).await?;
        if let Err(err) = session.quit().await {
            tracing::debug!(%err, "SMTP QUIT failed after the message was accepted");
        }
        Ok(receipt)
    }

    /// Sends one message; see [`OutgoingTransport::send`] for the error
    /// contract.
    pub async fn send_message(
        &self,
        envelope: &Envelope,
        message: &[u8],
    ) -> Result<SendReceipt, EngineError> {
        match self.attempt(envelope, message, false).await {
            Err(err) if err.code == ErrorCode::AuthFailed && self.auth.is_oauth() => {
                tracing::info!(host = %self.config.host, "SMTP rejected the access token; refreshing it once");
                self.attempt(envelope, message, true).await
            }
            other => other,
        }
    }
}

impl OutgoingTransport for SmtpTransport {
    fn send<'a>(
        &'a self,
        envelope: &'a Envelope,
        message: &'a [u8],
    ) -> BoxFuture<'a, Result<SendReceipt, EngineError>> {
        Box::pin(self.send_message(envelope, message))
    }
}

/// Connects, logs in and disconnects: the "test" step of account setup.
/// Never fails; problems are reported in the result, together with the
/// capabilities the server announced before the problem.
pub async fn test_login(config: &SmtpConfig, auth: &SmtpAuth) -> EndpointTestResult {
    let mut capabilities = Vec::new();
    let outcome: Result<(), EngineError> = async {
        let credentials = auth.credentials(false).await?;
        let mut session = SmtpSession::connect(config).await?;
        capabilities.clone_from(&session.capabilities().extensions);
        session.authenticate(&credentials).await?;
        drop(session.quit().await);
        Ok(())
    }
    .await;
    match outcome {
        Ok(()) => EndpointTestResult {
            error: None,
            message: None,
            capabilities,
            localized: None,
        },
        Err(err) => EndpointTestResult {
            error: Some(err.code),
            message: Some(err.message),
            capabilities,
            localized: err.localized.map(|l| *l),
        },
    }
}
