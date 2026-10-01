//! How the EWS client signs in: Basic over TLS (the password from the
//! keychain), OAuth2 bearer tokens (hybrid / Exchange Online; one forced
//! refresh on `401`) and Windows integrated sign-in (HTTP Negotiate:
//! Kerberos or NTLM through the injected
//! [`NegotiateAuth`] — SSPI lives in `nail-exchange`, so this crate has no
//! platform code).

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use nail_api::ErrorCode;
use nail_api::accounts::SecretRef;
use nail_api::engine::{AccessTokenProvider, EngineError, SecretStore};
use nail_api::exchange::engine::NegotiateAuth;

use crate::error::EwsError;

/// Where a Basic password comes from.
#[derive(Clone)]
pub enum Password {
    /// A fixed password (tests).
    Fixed(String),
    /// The keychain entry `secret` (read on first use, re-read once after
    /// the server refused it).
    Store {
        /// The secret store.
        store: Arc<dyn SecretStore>,
        /// The entry.
        secret: SecretRef,
    },
}

/// How the client signs in.
#[derive(Clone)]
pub enum EwsAuth {
    /// No `Authorization` header (only for loopback test servers).
    None,
    /// HTTP Basic (only over TLS, or to a loopback test server).
    Basic {
        /// The login name (`user@corp.example` or `CORP\user`).
        username: String,
        /// The password.
        password: Password,
    },
    /// A fixed OAuth2 bearer token (tests).
    Bearer(String),
    /// OAuth2 bearer tokens from the core's token manager
    /// (`EWS.AccessAsUser.All`), refreshed once when the server answers
    /// `401`.
    OAuth2 {
        /// The token source.
        tokens: Arc<dyn AccessTokenProvider>,
        /// The OAuth2 provider id (`microsoft`).
        provider: String,
        /// Where the refresh token is stored.
        secret: SecretRef,
    },
    /// HTTP Negotiate (Kerberos / NTLM). The context is bound to one
    /// connection, so the client keeps one connection per lane and
    /// serializes its requests.
    Negotiate(Arc<dyn NegotiateAuth>),
}

impl EwsAuth {
    /// Whether this is Windows integrated sign-in.
    pub fn is_negotiate(&self) -> bool {
        matches!(self, Self::Negotiate(_))
    }
}

impl fmt::Debug for EwsAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Basic { username, .. } => f
                .debug_struct("Basic")
                .field("username", username)
                .finish_non_exhaustive(),
            Self::Bearer(_) => f.write_str("Bearer(..)"),
            Self::OAuth2 { provider, .. } => f
                .debug_struct("OAuth2")
                .field("provider", provider)
                .finish_non_exhaustive(),
            Self::Negotiate(auth) => f.debug_tuple("Negotiate").field(auth).finish(),
        }
    }
}

/// Produces `Authorization` header values for the non-Negotiate logins,
/// caching the password read from the keychain.
#[derive(Debug, Default)]
pub(crate) struct HeaderSource {
    password: Mutex<Option<String>>,
}

impl HeaderSource {
    /// The `Authorization` value, or `None` for [`EwsAuth::None`] /
    /// [`EwsAuth::Negotiate`] (which the client handles itself).
    /// `refresh` re-reads the password / forces a new token.
    pub(crate) async fn header(
        &self,
        auth: &EwsAuth,
        refresh: bool,
    ) -> Result<Option<String>, EwsError> {
        Ok(match auth {
            EwsAuth::None | EwsAuth::Negotiate(_) => None,
            EwsAuth::Bearer(token) => Some(format!("Bearer {token}")),
            EwsAuth::OAuth2 {
                tokens,
                provider,
                secret,
            } => {
                let token = tokens
                    .access_token(provider, secret, refresh)
                    .await
                    .map_err(|e| EwsError::Credentials(Box::new(e)))?;
                Some(format!("Bearer {token}"))
            }
            EwsAuth::Basic { username, password } => {
                let password = self.password(password, refresh).await?;
                Some(basic_header(username, &password))
            }
        })
    }

    async fn password(&self, password: &Password, refresh: bool) -> Result<String, EwsError> {
        match password {
            Password::Fixed(p) => Ok(p.clone()),
            Password::Store { store, secret } => {
                if !refresh
                    && let Some(cached) = self
                        .password
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone()
                {
                    return Ok(cached);
                }
                let store = store.clone();
                let secret = secret.clone();
                let read = tokio::task::spawn_blocking(move || store.get(&secret))
                    .await
                    .map_err(|e| {
                        EwsError::Credentials(Box::new(EngineError::temporary(
                            ErrorCode::Internal,
                            e.to_string(),
                        )))
                    })?
                    .map_err(|e| EwsError::Credentials(Box::new(e)))?;
                let value = read.ok_or_else(|| {
                    EwsError::Credentials(Box::new(EngineError::permanent(
                        ErrorCode::AuthFailed,
                        "no password stored for the Exchange account",
                    )))
                })?;
                *self.password.lock().unwrap_or_else(PoisonError::into_inner) = Some(value.clone());
                Ok(value)
            }
        }
    }
}

/// `Basic base64(user:password)`.
pub fn basic_header(username: &str, password: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
    )
}

/// The Negotiate token of a `WWW-Authenticate` header value list
/// (`Negotiate <base64>`); `Some(None)` = the scheme without a token,
/// `None` = no Negotiate challenge at all.
pub fn negotiate_challenge<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> Option<Option<Vec<u8>>> {
    let mut found = None;
    for value in values {
        for part in value.split(',') {
            let part = part.trim();
            let (scheme, token) = part.split_once(' ').unwrap_or((part, ""));
            if scheme.eq_ignore_ascii_case("Negotiate") || scheme.eq_ignore_ascii_case("NTLM") {
                let token = token.trim();
                if token.is_empty() {
                    found.get_or_insert(None);
                } else if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(token) {
                    return Some(Some(bytes));
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::engine::MemorySecretStore;

    #[test]
    fn debug_hides_secrets() {
        let basic = EwsAuth::Basic {
            username: "ada".into(),
            password: Password::Fixed("hunter2".into()),
        };
        let text = format!("{basic:?} {:?}", EwsAuth::Bearer("tok-secret".into()));
        assert!(!text.contains("hunter2") && !text.contains("tok-secret"));
        assert!(text.contains("ada"));
        assert!(!basic.is_negotiate());
    }

    #[test]
    fn basic_headers() {
        assert_eq!(
            basic_header("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }

    #[test]
    fn negotiate_challenges() {
        assert_eq!(negotiate_challenge(["Basic realm=\"x\""]), None);
        assert_eq!(negotiate_challenge(["Negotiate", "NTLM"]), Some(None));
        assert_eq!(
            negotiate_challenge(["Negotiate YWJj"]),
            Some(Some(b"abc".to_vec()))
        );
        assert_eq!(
            negotiate_challenge(["Basic realm=\"x\", Negotiate YWJj"]),
            Some(Some(b"abc".to_vec()))
        );
    }

    #[tokio::test]
    async fn passwords_come_from_the_store_and_are_cached() {
        let store = Arc::new(MemorySecretStore::new());
        let secret = SecretRef::parse("keychain:nail/work/password").unwrap();
        let auth = EwsAuth::Basic {
            username: "ada".into(),
            password: Password::Store {
                store: store.clone(),
                secret: secret.clone(),
            },
        };
        let source = HeaderSource::default();
        let missing = source.header(&auth, false).await.unwrap_err();
        assert!(matches!(missing, EwsError::Credentials(_)));
        store.set(&secret, "one").unwrap();
        let first = source.header(&auth, false).await.unwrap().unwrap();
        assert_eq!(first, basic_header("ada", "one"));
        store.set(&secret, "two").unwrap();
        assert_eq!(source.header(&auth, false).await.unwrap().unwrap(), first);
        assert_eq!(
            source.header(&auth, true).await.unwrap().unwrap(),
            basic_header("ada", "two")
        );
        assert_eq!(source.header(&EwsAuth::None, false).await.unwrap(), None);
    }
}
