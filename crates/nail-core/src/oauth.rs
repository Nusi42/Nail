//! OAuth2: the token source every engine uses ([`Tokens`], a
//! [`TokenManager`] rebuilt when the providers in `accounts.kdl` change)
//! and the sign-in flows started by `StartOAuth` ([`OAuthFlows`]).
//!
//! A finished flow's tokens stay in memory, keyed by its flow id, until a
//! request references the flow (`SecretValue::OAuth2 {flow_id}`) or the
//! flow expires after 10 minutes. Only then is the refresh token written
//! to the keychain.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

use nail_api::accounts::OAuthProviderConfig;
use nail_api::engine::{AccessTokenProvider, BoxFuture, EngineError, SecretStore};
use nail_api::{ApiError, ErrorCode, OAuthFlowId};
use nail_auth::{FlowCanceller, ProviderRegistry, TokenManager, TokenSet};

/// How long a finished flow's tokens wait to be claimed.
pub(crate) const FLOW_TTL: Duration = Duration::from_secs(10 * 60);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The OAuth2 token source of all accounts.
pub(crate) struct Tokens {
    current: RwLock<Arc<TokenManager>>,
    providers: Mutex<Vec<OAuthProviderConfig>>,
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("manager", &self.manager())
            .finish_non_exhaustive()
    }
}

impl Tokens {
    /// A token source for the providers of `providers`.
    pub(crate) fn new(
        providers: &[OAuthProviderConfig],
        secrets: Arc<dyn SecretStore>,
        http: reqwest::Client,
    ) -> Self {
        let manager = Self::build(providers, &secrets, &http);
        Self {
            current: RwLock::new(Arc::new(manager)),
            providers: Mutex::new(providers.to_vec()),
            secrets,
            http,
        }
    }

    fn build(
        providers: &[OAuthProviderConfig],
        secrets: &Arc<dyn SecretStore>,
        http: &reqwest::Client,
    ) -> TokenManager {
        let registry = ProviderRegistry::from_config(providers).unwrap_or_else(|err| {
            tracing::warn!(%err, "invalid OAuth2 providers; using the built-in ones only");
            ProviderRegistry::default()
        });
        TokenManager::new(Arc::new(registry), Arc::clone(secrets), http.clone())
    }

    /// The current manager.
    pub(crate) fn manager(&self) -> Arc<TokenManager> {
        Arc::clone(&self.current.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Rebuilds the manager when the providers changed (cached access
    /// tokens are dropped then; refresh tokens stay in the keychain).
    pub(crate) fn reconfigure(&self, providers: &[OAuthProviderConfig]) {
        {
            let mut current = lock(&self.providers);
            if current.as_slice() == providers {
                return;
            }
            *current = providers.to_vec();
        }
        let manager = Arc::new(Self::build(providers, &self.secrets, &self.http));
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = manager;
    }
}

impl AccessTokenProvider for Tokens {
    fn access_token<'a>(
        &'a self,
        provider: &'a str,
        secret: &'a nail_api::SecretRef,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        let manager = self.manager();
        Box::pin(async move {
            manager
                .token(provider, secret, force_refresh)
                .await
                .map_err(EngineError::from)
        })
    }
}

enum FlowState {
    Pending {
        provider: String,
        canceller: FlowCanceller,
    },
    Done {
        provider: String,
        tokens: TokenSet,
        at: Instant,
    },
}

/// Sign-in flows started by `StartOAuth`.
#[derive(Default)]
pub(crate) struct OAuthFlows {
    next: AtomicU64,
    flows: Mutex<HashMap<OAuthFlowId, FlowState>>,
}

impl std::fmt::Debug for OAuthFlows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthFlows")
            .field("flows", &lock(&self.flows).len())
            .finish()
    }
}

impl OAuthFlows {
    /// A fresh flow id.
    pub(crate) fn next_id(&self) -> OAuthFlowId {
        OAuthFlowId::new(format!(
            "oauth-{}",
            self.next.fetch_add(1, Ordering::Relaxed) + 1
        ))
    }

    /// Registers a started flow.
    pub(crate) fn started(&self, id: OAuthFlowId, provider: String, canceller: FlowCanceller) {
        self.expire();
        lock(&self.flows).insert(
            id,
            FlowState::Pending {
                provider,
                canceller,
            },
        );
    }

    /// Records a flow's outcome: tokens are kept; a failed flow is dropped.
    pub(crate) fn finished(&self, id: &OAuthFlowId, tokens: Option<TokenSet>) {
        let mut flows = lock(&self.flows);
        if let (Some(FlowState::Pending { provider, .. }), Some(tokens)) =
            (flows.remove(id), tokens)
        {
            flows.insert(
                id.clone(),
                FlowState::Done {
                    provider,
                    tokens,
                    at: Instant::now(),
                },
            );
        }
    }

    /// Cancels a running flow (it finishes with `cancelled`).
    pub(crate) fn cancel(&self, id: &OAuthFlowId) -> Result<(), ApiError> {
        match lock(&self.flows).get(id) {
            Some(FlowState::Pending { canceller, .. }) => {
                canceller.cancel();
                Ok(())
            }
            Some(FlowState::Done { .. }) => Err(ApiError::new(
                ErrorCode::Conflict,
                format!("sign-in {id} already finished"),
            )),
            None => Err(ApiError::not_found(format!("sign-in {id}"))),
        }
    }

    /// The provider and tokens of a finished flow, without claiming them.
    pub(crate) fn peek(&self, id: &OAuthFlowId) -> Result<(String, TokenSet), ApiError> {
        self.expire();
        match lock(&self.flows).get(id) {
            Some(FlowState::Done {
                provider, tokens, ..
            }) => Ok((provider.clone(), tokens.clone())),
            Some(FlowState::Pending { .. }) => Err(ApiError::new(
                ErrorCode::Unavailable,
                format!("sign-in {id} has not finished yet"),
            )),
            None => Err(ApiError::not_found(format!(
                "sign-in {id} (unknown, failed or expired)"
            ))),
        }
    }

    /// Claims a finished flow's tokens (they can be used once).
    pub(crate) fn take(&self, id: &OAuthFlowId) -> Result<(String, TokenSet), ApiError> {
        let found = self.peek(id)?;
        lock(&self.flows).remove(id);
        Ok(found)
    }

    fn expire(&self) {
        lock(&self.flows).retain(|_, state| match state {
            FlowState::Done { at, .. } => at.elapsed() < FLOW_TTL,
            FlowState::Pending { .. } => true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> TokenSet {
        TokenSet {
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            expires_at: Instant::now() + Duration::from_secs(3600),
            scopes: vec![],
        }
    }

    #[tokio::test]
    async fn flows_hold_tokens_until_claimed() {
        let flows = OAuthFlows::default();
        let id = flows.next_id();
        assert_eq!(id.as_str(), "oauth-1");
        assert_eq!(flows.next_id().as_str(), "oauth-2");
        assert_eq!(
            flows.peek(&id).unwrap_err().code,
            ErrorCode::NotFound,
            "unknown before it starts"
        );
        // A canceller from a real (loopback) flow is not needed here: a
        // finished flow is recorded directly.
        lock(&flows.flows).insert(
            id.clone(),
            FlowState::Done {
                provider: "google".into(),
                tokens: tokens(),
                at: Instant::now(),
            },
        );
        assert_eq!(flows.peek(&id).unwrap().0, "google");
        assert_eq!(flows.cancel(&id).unwrap_err().code, ErrorCode::Conflict);
        assert_eq!(flows.take(&id).unwrap().1.access_token, "at");
        assert_eq!(flows.take(&id).unwrap_err().code, ErrorCode::NotFound);
        flows.finished(&id, Some(tokens()));
        assert!(flows.peek(&id).is_err(), "only pending flows can finish");
    }

    #[test]
    fn token_source_rebuilds() {
        let secrets: Arc<dyn SecretStore> = Arc::new(nail_api::engine::MemorySecretStore::new());
        let tokens = Tokens::new(&[], secrets, nail_auth::http_client().unwrap());
        let before = tokens.manager();
        tokens.reconfigure(&[]);
        assert!(Arc::ptr_eq(&before, &tokens.manager()), "unchanged: kept");
        tokens.reconfigure(&[OAuthProviderConfig {
            id: "google".into(),
            client_id: "cid".into(),
            client_secret: None,
            auth_url: None,
            token_url: None,
            scopes: vec![],
        }]);
        assert!(!Arc::ptr_eq(&before, &tokens.manager()));
        assert!(tokens.manager().registry().ready("google").is_ok());
    }
}
