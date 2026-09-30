//! Where the ManageSieve login comes from: the account's IMAP login
//! ([`nail_api::sieve::ManageSieveConfig`] never has its own), with the
//! password from the keychain or an OAuth2 access token (refreshed once
//! when the server rejects it).

use std::fmt;
use std::sync::Arc;

use nail_api::ErrorCode;
use nail_api::accounts::{AuthConfig, SaslMechanism, SecretRef};
use nail_api::engine::{AccessTokenProvider, EngineError, SecretStore};
use nail_api::ids::AccountId;

use crate::client::{SieveConfig, SieveCredentials, SieveSession};

/// A credential source, resolved per connection.
#[derive(Clone)]
pub enum SieveAuth {
    /// Fixed credentials (tests, previews).
    Credentials(SieveCredentials),
    /// A password read from the keychain on every connection.
    StoredPassword {
        /// Login name.
        username: String,
        /// Keychain reference.
        secret: SecretRef,
        /// Secret storage.
        store: Arc<dyn SecretStore>,
    },
    /// An OAuth2 access token from the token provider.
    OAuth2 {
        /// Login name.
        username: String,
        /// Provider id.
        provider: String,
        /// Keychain reference of the refresh token.
        secret: SecretRef,
        /// Token source.
        tokens: Arc<dyn AccessTokenProvider>,
        /// Forced mechanism, else automatic.
        mechanism: Option<SaslMechanism>,
    },
}

impl fmt::Debug for SieveAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

impl SieveAuth {
    /// The source for an account's IMAP [`AuthConfig`].
    pub fn from_config(
        account: &AccountId,
        auth: &AuthConfig,
        store: Arc<dyn SecretStore>,
        tokens: Arc<dyn AccessTokenProvider>,
    ) -> Result<Self, EngineError> {
        match (auth, auth.secret_ref(account)) {
            (AuthConfig::Password { username, .. }, Some(secret)) => Ok(Self::StoredPassword {
                username: username.clone(),
                secret,
                store,
            }),
            (
                AuthConfig::OAuth2 {
                    username,
                    provider,
                    mechanism,
                    ..
                },
                Some(secret),
            ) => Ok(Self::OAuth2 {
                username: username.clone(),
                provider: provider.clone(),
                secret,
                tokens,
                mechanism: *mechanism,
            }),
            _ => Err(EngineError::permanent(
                ErrorCode::Unsupported,
                "ManageSieve needs a password or OAuth2 login",
            )),
        }
    }

    fn is_oauth(&self) -> bool {
        matches!(
            self,
            Self::OAuth2 { .. } | Self::Credentials(SieveCredentials::OAuth2 { .. })
        )
    }

    /// The credentials for one login.
    pub async fn credentials(&self, force_refresh: bool) -> Result<SieveCredentials, EngineError> {
        match self {
            Self::Credentials(c) => Ok(c.clone()),
            Self::StoredPassword {
                username,
                secret,
                store,
            } => {
                let password = store.get(secret)?.ok_or_else(|| {
                    EngineError::permanent(
                        ErrorCode::AuthFailed,
                        format!("no password stored under {secret}"),
                    )
                })?;
                Ok(SieveCredentials::password(username.clone(), password))
            }
            Self::OAuth2 {
                username,
                provider,
                secret,
                tokens,
                mechanism,
            } => {
                let access_token = tokens.access_token(provider, secret, force_refresh).await?;
                Ok(SieveCredentials::OAuth2 {
                    username: username.clone(),
                    access_token,
                    mechanism: *mechanism,
                })
            }
        }
    }

    /// Connects and logs in; a rejected OAuth2 token is refreshed and the
    /// login retried once.
    pub async fn login(&self, config: &SieveConfig) -> Result<SieveSession, EngineError> {
        let credentials = self.credentials(false).await?;
        match SieveSession::login(config, &credentials).await {
            Err(err) if err.code() == ErrorCode::AuthFailed && self.is_oauth() => {
                tracing::info!(host = %config.host, "ManageSieve rejected the access token; refreshing it once");
                let credentials = self.credentials(true).await?;
                Ok(SieveSession::login(config, &credentials).await?)
            }
            other => Ok(other?),
        }
    }
}
