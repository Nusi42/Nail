//! [`TokenManager`]: starts sign-in flows, stores refresh tokens through a
//! [`SecretStore`] and hands out (cached, refreshed) access tokens as the
//! core's [`AccessTokenProvider`].

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::SecretRef;
use nail_api::engine::{AccessTokenProvider, BoxFuture, EngineError, SecretStore};

use crate::client::{OAuthClient, TokenSet};
use crate::error::AuthError;
use crate::flow::{FlowOptions, PendingFlow, start_flow};
use crate::providers::ProviderRegistry;

/// An access token is refreshed when it expires within this margin.
pub const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// OAuth2 token management for all accounts.
pub struct TokenManager {
    registry: Arc<ProviderRegistry>,
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
    cache: Mutex<HashMap<SecretRef, TokenSet>>,
    /// One refresh at a time per refresh token (single flight).
    locks: Mutex<HashMap<SecretRef, Arc<tokio::sync::Mutex<()>>>>,
}

impl fmt::Debug for TokenManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut cached: Vec<String> = lock(&self.cache).keys().map(ToString::to_string).collect();
        cached.sort();
        f.debug_struct("TokenManager")
            .field(
                "providers",
                &self
                    .registry
                    .providers()
                    .iter()
                    .map(|p| &p.id)
                    .collect::<Vec<_>>(),
            )
            .field("cached", &cached)
            .finish_non_exhaustive()
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, EngineError> + Send + 'static,
) -> Result<T, EngineError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| EngineError::temporary(ErrorCode::Internal, e.to_string()))?
}

impl TokenManager {
    /// A manager for the providers of `registry`, keeping refresh tokens in
    /// `secrets` and talking HTTP through `http` (see [`crate::http_client`]).
    pub fn new(
        registry: Arc<ProviderRegistry>,
        secrets: Arc<dyn SecretStore>,
        http: reqwest::Client,
    ) -> Self {
        Self {
            registry,
            secrets,
            http,
            cache: Mutex::default(),
            locks: Mutex::default(),
        }
    }

    /// The provider registry.
    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    /// A ready client for `provider`, with its client secret read from the
    /// keychain.
    pub async fn client(&self, provider: &str) -> Result<OAuthClient, AuthError> {
        let spec = self.registry.ready(provider)?.clone();
        let client_secret = match &spec.client_secret {
            None => None,
            Some(reference) => {
                let store = Arc::clone(&self.secrets);
                let r = reference.clone();
                let value = blocking(move || store.get(&r)).await?;
                Some(value.ok_or_else(|| {
                    AuthError::Config(format!(
                        "the client secret of provider {provider:?} is not in the keychain ({reference})"
                    ))
                })?)
            }
        };
        OAuthClient::new(spec, client_secret, self.http.clone())
    }

    /// Starts a sign-in flow for `provider` (see [`start_flow`]).
    pub async fn start_flow(
        &self,
        provider: &str,
        options: FlowOptions,
    ) -> Result<PendingFlow, AuthError> {
        start_flow(self.client(provider).await?, options).await
    }

    /// Stores the tokens of a finished flow under `secret`: the refresh
    /// token in the keychain, the access token in memory.
    pub async fn store(&self, secret: &SecretRef, tokens: &TokenSet) -> Result<(), AuthError> {
        let refresh = tokens.refresh_token.clone().ok_or_else(|| {
            AuthError::Protocol(
                "the provider issued no refresh token (offline access not granted)".to_owned(),
            )
        })?;
        let store = Arc::clone(&self.secrets);
        let r = secret.clone();
        blocking(move || store.set(&r, &refresh)).await?;
        lock(&self.cache).insert(secret.clone(), tokens.clone());
        Ok(())
    }

    /// Drops the cached access token of `secret` (e.g. when the account is
    /// removed; deleting the keychain entry is the caller's job).
    pub fn forget(&self, secret: &SecretRef) {
        lock(&self.cache).remove(secret);
        lock(&self.locks).remove(secret);
    }

    fn cached(&self, secret: &SecretRef) -> Option<String> {
        lock(&self.cache)
            .get(secret)
            .filter(|t| t.is_fresh(EXPIRY_MARGIN))
            .map(|t| t.access_token.clone())
    }

    /// A valid access token (see [`AccessTokenProvider::access_token`]).
    pub async fn token(
        &self,
        provider: &str,
        secret: &SecretRef,
        force_refresh: bool,
    ) -> Result<String, AuthError> {
        if !force_refresh && let Some(token) = self.cached(secret) {
            return Ok(token);
        }
        let flight = Arc::clone(lock(&self.locks).entry(secret.clone()).or_default());
        let _guard = flight.lock().await;
        // Someone else may have refreshed while we waited.
        if !force_refresh && let Some(token) = self.cached(secret) {
            return Ok(token);
        }
        let store = Arc::clone(&self.secrets);
        let r = secret.clone();
        let refresh_token = blocking(move || store.get(&r)).await?.ok_or_else(|| {
            AuthError::InvalidGrant(format!("no refresh token stored under {secret}"))
        })?;
        let client = self.client(provider).await?;
        let mut tokens = match client.refresh(&refresh_token).await {
            Ok(tokens) => tokens,
            Err(err) => {
                if matches!(err, AuthError::InvalidGrant(_)) {
                    lock(&self.cache).remove(secret);
                }
                tracing::info!(%provider, %secret, error = %err, "OAuth2 refresh failed");
                return Err(err);
            }
        };
        match &tokens.refresh_token {
            Some(rotated) if *rotated != refresh_token => {
                let store = Arc::clone(&self.secrets);
                let r = secret.clone();
                let rotated = rotated.clone();
                blocking(move || store.set(&r, &rotated)).await?;
            }
            _ => tokens.refresh_token = Some(refresh_token),
        }
        tracing::debug!(%provider, %secret, "OAuth2 access token refreshed");
        let access = tokens.access_token.clone();
        lock(&self.cache).insert(secret.clone(), tokens);
        Ok(access)
    }
}

impl AccessTokenProvider for TokenManager {
    fn access_token<'a>(
        &'a self,
        provider: &'a str,
        secret: &'a SecretRef,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move {
            self.token(provider, secret, force_refresh)
                .await
                .map_err(EngineError::from)
        })
    }
}
