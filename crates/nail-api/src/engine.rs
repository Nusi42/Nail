//! Engine-facing traits (Rust only, not part of the wire protocol).
//!
//! The core (`nail-core`) wires protocol and platform crates together. The
//! seams that more than one crate implements or consumes live here, in the
//! crate every other crate already depends on, so that `nail-proto-smtp`,
//! `nail-auth` and `nail-core` do not have to depend on each other:
//!
//! | Trait | Implemented by | Used by |
//! | --- | --- | --- |
//! | [`SecretStore`] | `nail-auth` (OS keychain), [`MemorySecretStore`] (tests) | `nail-core` account manager, `nail-auth` token cache |
//! | [`OutgoingTransport`] | `nail-proto-smtp` (later: Graph, Gmail API) | `nail-core` outbox |
//! | [`AccessTokenProvider`] | `nail-auth` | `nail-core` when connecting IMAP / SMTP with OAuth2 |
//!
//! Replaying queued writes uses `nail_store::Replayer`, implemented per
//! protocol by `nail-core`. See `docs/adr/0012-m1a-contract.md`.
//!
//! The traits are object-safe (`Arc<dyn SecretStore>` etc.); async methods
//! return a [`BoxFuture`].

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};

use crate::accounts::SecretRef;
use crate::error::{ApiError, ErrorCode};
use crate::localized::LocalizedMessage;

/// A boxed, `Send` future, as returned by the async trait methods here.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An error from an engine: an [`ErrorCode`] for the UI, an English
/// diagnostic and whether trying again later may succeed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct EngineError {
    /// Machine-readable error class.
    pub code: ErrorCode,
    /// Diagnostic detail (English, for logs). Must never contain secrets.
    pub message: String,
    /// `true` for temporary failures (network down, SMTP 4xx, keychain
    /// locked): the caller keeps the work queued and retries. `false` for
    /// permanent ones (SMTP 5xx, rejected credentials, message too large).
    pub retryable: bool,
    /// The user-facing text (Fluent id + arguments), when the engine has a
    /// more specific one than the error class (ADR 0043).
    pub localized: Option<Box<LocalizedMessage>>,
}

impl EngineError {
    /// A permanent error.
    pub fn permanent(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            localized: None,
        }
    }

    /// A temporary error.
    pub fn temporary(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: true,
            localized: None,
        }
    }

    /// Attaches the user-facing text (Fluent id + arguments).
    #[must_use]
    pub fn with_localized(mut self, localized: LocalizedMessage) -> Self {
        self.localized = Some(Box::new(localized));
        self
    }
}

impl From<EngineError> for ApiError {
    fn from(err: EngineError) -> Self {
        ApiError {
            code: err.code,
            message: err.message,
            localized: err.localized,
        }
    }
}

// ---------------------------------------------------------------------------
// Secrets

/// Stores secrets (passwords, OAuth2 refresh tokens, client certificates)
/// addressed by [`SecretRef`].
///
/// Methods are blocking (OS keychain calls are short system calls); call them
/// from `tokio::task::spawn_blocking` in async code. Implementations map a
/// reference `keychain:<service>/<name>` to the platform entry (service,
/// user) = (`<service>`, `<name>`).
pub trait SecretStore: Send + Sync + fmt::Debug {
    /// The secret, or `None` when no entry exists.
    fn get(&self, secret: &SecretRef) -> Result<Option<String>, EngineError>;
    /// Creates or replaces the secret.
    fn set(&self, secret: &SecretRef, value: &str) -> Result<(), EngineError>;
    /// Deletes the secret. Returns whether an entry existed.
    fn delete(&self, secret: &SecretRef) -> Result<bool, EngineError>;
}

/// An in-memory [`SecretStore`] for tests, demos and the web mock.
#[derive(Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<SecretRef, String>>,
}

impl MemorySecretStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored secrets.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SecretRef, String>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for MemorySecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut keys: Vec<String> = self.lock().keys().map(ToString::to_string).collect();
        keys.sort();
        f.debug_struct("MemorySecretStore")
            .field("refs", &keys)
            .finish()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, secret: &SecretRef) -> Result<Option<String>, EngineError> {
        Ok(self.lock().get(secret).cloned())
    }

    fn set(&self, secret: &SecretRef, value: &str) -> Result<(), EngineError> {
        self.lock().insert(secret.clone(), value.to_owned());
        Ok(())
    }

    fn delete(&self, secret: &SecretRef) -> Result<bool, EngineError> {
        Ok(self.lock().remove(secret).is_some())
    }
}

// ---------------------------------------------------------------------------
// OAuth2 access tokens

/// Hands out OAuth2 access tokens for IMAP / SMTP logins (XOAUTH2,
/// OAUTHBEARER), refreshing them with the refresh token stored under a
/// [`SecretRef`].
pub trait AccessTokenProvider: Send + Sync + fmt::Debug {
    /// A valid access token for `provider` (an
    /// [`crate::accounts::OAuthProviderConfig::id`]) from the refresh token
    /// in `secret`. `force_refresh` skips any cached token (use it after the
    /// server rejected one). A revoked or missing refresh token is
    /// [`ErrorCode::AuthFailed`], not retryable.
    fn access_token<'a>(
        &'a self,
        provider: &'a str,
        secret: &'a SecretRef,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>>;
}

// ---------------------------------------------------------------------------
// Sending

/// The SMTP envelope of a message: who the server reports bounces to and who
/// receives it (including `Bcc` recipients, which are not in the headers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// `MAIL FROM` address.
    pub from: String,
    /// `RCPT TO` addresses (at least one).
    pub to: Vec<String>,
}

/// What the server said when it accepted a message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SendReceipt {
    /// The server's final reply text (e.g. `250 2.0.0 Ok: queued as 12345`),
    /// for logs.
    pub response: String,
}

/// Submits finished RFC 5322 messages. One instance serves one account's
/// outgoing server; the core builds it from the account's
/// [`crate::accounts::OutgoingServer`] and credentials.
pub trait OutgoingTransport: Send + Sync + fmt::Debug {
    /// Sends `message` (the complete raw MIME, CRLF line endings, `Bcc`
    /// header already removed) to the envelope's recipients.
    ///
    /// Errors: [`ErrorCode::AuthFailed`] (not retryable),
    /// [`ErrorCode::TooLarge`] (the server's `SIZE` limit, not retryable),
    /// [`ErrorCode::Network`] / [`ErrorCode::Tls`] (retryable), SMTP 4xx
    /// replies (retryable) and 5xx replies (not retryable, code
    /// [`ErrorCode::Rejected`]).
    fn send<'a>(
        &'a self,
        envelope: &'a Envelope,
        message: &'a [u8],
    ) -> BoxFuture<'a, Result<SendReceipt, EngineError>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn memory_secret_store_round_trip() {
        let store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let r = SecretRef::parse("keychain:nail/work/password").unwrap();
        assert_eq!(store.get(&r).unwrap(), None);
        store.set(&r, "hunter2").unwrap();
        assert_eq!(store.get(&r).unwrap().as_deref(), Some("hunter2"));
        assert!(!format!("{store:?}").contains("hunter2"));
        assert!(store.delete(&r).unwrap());
        assert!(!store.delete(&r).unwrap());
    }

    #[derive(Debug)]
    struct Recorder(Mutex<Vec<(Envelope, Vec<u8>)>>);

    impl OutgoingTransport for Recorder {
        fn send<'a>(
            &'a self,
            envelope: &'a Envelope,
            message: &'a [u8],
        ) -> BoxFuture<'a, Result<SendReceipt, EngineError>> {
            Box::pin(async move {
                if envelope.to.is_empty() {
                    return Err(EngineError::permanent(
                        ErrorCode::InvalidRequest,
                        "no recipients",
                    ));
                }
                self.0
                    .lock()
                    .unwrap()
                    .push((envelope.clone(), message.to_vec()));
                Ok(SendReceipt {
                    response: "250 ok".into(),
                })
            })
        }
    }

    /// Polls a future that never waits (no runtime needed).
    fn block_on<F: Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let mut fut = std::pin::pin!(fut);
        let mut cx = Context::from_waker(Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(out) => out,
            Poll::Pending => panic!("future was not ready"),
        }
    }

    #[test]
    fn outgoing_transport_is_object_safe() {
        let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
        let transport: Arc<dyn OutgoingTransport> = recorder.clone();
        let envelope = Envelope {
            from: "ada@example.org".into(),
            to: vec!["bob@example.org".into()],
        };
        let receipt = block_on(transport.send(&envelope, b"Subject: hi\r\n\r\nHi\r\n")).unwrap();
        assert_eq!(receipt.response, "250 ok");
        assert_eq!(recorder.0.lock().unwrap().len(), 1);

        let err = block_on(transport.send(
            &Envelope {
                from: "a@b".into(),
                to: vec![],
            },
            b"",
        ))
        .unwrap_err();
        assert!(!err.retryable);
        let api: ApiError = err.into();
        assert_eq!(api.code, ErrorCode::InvalidRequest);
        assert!(EngineError::temporary(ErrorCode::Network, "down").retryable);
    }
}
