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
pub const EXPIRY_MARGIN: Duration = Duration::from_mins(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where the token requests of each refresh token are logged (ADR 0190):
/// the core registers its accounts' references with their account's tap.
/// Sign-in flows (no refresh token yet) log into the `flows` tap.
#[derive(Debug, Default)]
pub struct OAuthTaps {
    secrets: Mutex<HashMap<SecretRef, nail_connlog::HttpTap>>,
    flows: Mutex<nail_connlog::HttpTap>,
}

impl OAuthTaps {
    /// Logs the token requests of `secret` into `tap`.
    pub fn set(&self, secret: &SecretRef, tap: nail_connlog::HttpTap) {
        lock(&self.secrets).insert(secret.clone(), tap);
    }

    /// The tap of `secret` (off when none is registered).
    pub fn get(&self, secret: &SecretRef) -> nail_connlog::HttpTap {
        lock(&self.secrets).get(secret).cloned().unwrap_or_default()
    }

    /// Logs the code exchanges of sign-in flows into `tap`.
    pub fn set_flows(&self, tap: nail_connlog::HttpTap) {
        *lock(&self.flows) = tap;
    }

    /// The tap of sign-in flows.
    pub fn flows(&self) -> nail_connlog::HttpTap {
        lock(&self.flows).clone()
    }
}

/// OAuth2 token management for all accounts.
pub struct TokenManager {
    registry: Arc<ProviderRegistry>,
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
    cache: Mutex<HashMap<SecretRef, TokenSet>>,
    /// One refresh at a time per refresh token (single flight).
    locks: Mutex<HashMap<SecretRef, Arc<tokio::sync::Mutex<()>>>>,
    /// The connection logs of the token requests.
    taps: Arc<OAuthTaps>,
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
            taps: Arc::default(),
        }
    }

    /// Logs token requests into the taps registered in `taps` (ADR 0190).
    #[must_use]
    pub fn with_taps(mut self, taps: Arc<OAuthTaps>) -> Self {
        self.taps = taps;
        self
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
        let client = self.client(provider).await?.with_tap(self.taps.flows());
        start_flow(client, options).await
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
        let tap = self.taps.get(secret);
        let client = self.client(provider).await?.with_tap(tap.clone());
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
        tap.add_secret(&access);
        if let Some(rotated) = &tokens.refresh_token {
            tap.add_secret(rotated);
        }
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
