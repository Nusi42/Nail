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
//! | [`MailApiEngine`] (M3b) | `nail-proto-gmail` (M3c: Graph, EWS) | `nail-core` sync loop and op replay of API accounts |
//! | [`FreeBusyProvider`] (M3b) | `nail-dav` (schedule outbox; M3c: Exchange) | `nail-core` free/busy queries |
//!
//! # API engines (M3b, ADR 0065)
//!
//! IMAP engines are driven by `nail-core` directly (folders, UIDs,
//! UIDVALIDITY, IDLE). Mail servers reached through a REST API (Gmail; in
//! M3c Microsoft Graph and EWS) instead implement the provider-neutral
//! [`MailApiEngine`]: messages have an opaque **remote id** (stored in
//! `messages.remote_id`, kept across moves), a sync **cursor** (Gmail
//! `historyId`, Graph delta link, EWS sync state) replaces UIDs and MODSEQs,
//! a cursor the server no longer knows ([`DeltaOutcome::CursorExpired`])
//! means a full resync, and queued writes replay as [`RemoteWrite`]s.
//! There is no push: the core polls with [`AdaptivePoll`] and honours
//! [`EngineError::retry_after_secs`] (quotas, HTTP 429 / 503).
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
use crate::domain::{Flags, FolderRole};
use crate::error::{ApiError, ErrorCode};
use crate::gmail::GmailMessageInfo;
use crate::localized::LocalizedMessage;
use crate::ops::FlagChange;
use crate::scheduling::{AttendeeFreeBusy, FreeBusyQuery};

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
    /// The server asked to wait this long before the next request
    /// (`Retry-After`, quota exhausted; M3b). `None` = the caller's own
    /// backoff.
    pub retry_after_secs: Option<u32>,
}

impl EngineError {
    /// A permanent error.
    pub fn permanent(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            localized: None,
            retry_after_secs: None,
        }
    }

    /// A temporary error.
    pub fn temporary(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: true,
            localized: None,
            retry_after_secs: None,
        }
    }

    /// Attaches the user-facing text (Fluent id + arguments).
    #[must_use]
    pub fn with_localized(mut self, localized: LocalizedMessage) -> Self {
        self.localized = Some(Box::new(localized));
        self
    }

    /// A temporary "slow down" error ([`ErrorCode::RateLimited`]) asking
    /// the caller to wait `secs` seconds (M3b).
    pub fn rate_limited(message: impl Into<String>, secs: u32) -> Self {
        Self::temporary(ErrorCode::RateLimited, message).with_retry_after(secs)
    }

    /// Records the server's `Retry-After` (seconds).
    #[must_use]
    pub fn with_retry_after(mut self, secs: u32) -> Self {
        self.retry_after_secs = Some(secs);
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

// ---------------------------------------------------------------------------
// API mail engines (M3b, ADR 0065)

/// A folder (or folder-like label) on an API server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFolder {
    /// The server's id (Gmail: the folder path of
    /// [`crate::gmail::GmailFolder`]; Graph / EWS: the folder id). Stored
    /// as the local folder's path.
    pub remote_id: String,
    /// Display name.
    pub name: String,
    /// Special-use role.
    pub role: FolderRole,
    /// The parent's remote id.
    pub parent: Option<String>,
}

/// A message as an API server lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessage {
    /// The server's message id (`messages.remote_id`).
    pub remote_id: String,
    /// The server's thread / conversation id, if it has one.
    pub thread_id: Option<String>,
    /// The remote id of the folder it is in ([`RemoteFolder::remote_id`]).
    pub folder: String,
    /// System flags.
    pub flags: Flags,
    /// Tags (Gmail: user label names; Exchange: categories).
    pub tags: Vec<String>,
    /// The server's receive time (Unix seconds).
    pub internal_date: Option<i64>,
    /// Size of the raw message in bytes.
    pub size: Option<u64>,
    /// The raw RFC 5322 message, when the listing included it; `None` =
    /// fetch with [`MailApiEngine::fetch_raw`] (or headers-only policy).
    pub raw: Option<Vec<u8>>,
    /// Gmail-specific data (labels, category).
    pub gmail: Option<GmailMessageInfo>,
}

/// A metadata change of a known message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessageChange {
    /// The message.
    pub remote_id: String,
    /// Its folder now, when it moved.
    pub folder: Option<String>,
    /// Its flags now, when they changed.
    pub flags: Option<Flags>,
    /// Its tags now, when they changed.
    pub tags: Option<Vec<String>>,
    /// Gmail data now, when labels changed.
    pub gmail: Option<GmailMessageInfo>,
}

/// One page of a full listing ([`MailApiEngine::full_sync_page`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FullSyncPage {
    /// The cursor to continue with incremental sync once every page is
    /// stored; set on the first page (taken before listing, so nothing
    /// that changes during the listing is lost).
    pub cursor: Option<String>,
    /// Messages of this page, newest first.
    pub messages: Vec<RemoteMessage>,
    /// The token of the next page; `None` = done.
    pub next_page: Option<String>,
}

/// Changes since a cursor ([`MailApiEngine::changes`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncDelta {
    /// The cursor after these changes. Store it only after the changes.
    pub cursor: String,
    /// New messages.
    pub added: Vec<RemoteMessage>,
    /// Changed messages.
    pub changed: Vec<RemoteMessageChange>,
    /// Remote ids of messages that are gone (deleted permanently).
    pub removed: Vec<String>,
    /// More changes wait: call again with `cursor`.
    pub more: bool,
}

/// The answer to [`MailApiEngine::changes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeltaOutcome {
    /// The changes.
    Delta(SyncDelta),
    /// The server no longer knows the cursor (Gmail: `historyId` too old,
    /// HTTP 404): list everything again with
    /// [`MailApiEngine::full_sync_page`] and merge.
    CursorExpired,
}

/// A queued write as an API engine replays it (the provider-neutral form
/// of the store's operations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteWrite {
    /// Set flags and tags (Gmail: `messages.modify` with labels).
    SetFlags {
        /// The message.
        remote_id: String,
        /// The change (absolute, idempotent).
        change: FlagChange,
    },
    /// Move between folders (Gmail: a label change, see
    /// [`crate::gmail::GmailFolder::move_change`]).
    Move {
        /// The message.
        remote_id: String,
        /// The folder it was in.
        from_folder: String,
        /// The target folder.
        to_folder: String,
    },
    /// Move to the trash (Gmail: `messages.trash`).
    Trash {
        /// The message.
        remote_id: String,
    },
    /// Delete permanently (Gmail: `messages.delete`).
    Delete {
        /// The message.
        remote_id: String,
    },
    /// Store a message that exists only locally (Gmail:
    /// `messages.insert`).
    Append {
        /// Target folder.
        folder: String,
        /// Raw RFC 5322 bytes.
        raw: Vec<u8>,
        /// Its flags.
        flags: Flags,
    },
}

/// What a replayed [`RemoteWrite`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Done; an `Append` reports the new remote id.
    Done {
        /// The new message's remote id (`Append` only).
        remote_id: Option<String>,
    },
    /// The message no longer exists on the server (the write is dropped,
    /// the next sync removes the local copy).
    Gone,
}

/// A mail engine reached through a REST API (Gmail; M3c Graph, EWS). One
/// instance serves one account; the core builds it from the account's
/// configuration and an [`AccessTokenProvider`], runs the sync loop
/// (full listing, then [`Self::changes`] at [`AdaptivePoll`] intervals),
/// replays queued writes through [`Self::write`] and sends through the
/// engine's [`OutgoingTransport`].
///
/// Errors: [`ErrorCode::AuthFailed`] (token revoked; not retryable),
/// [`ErrorCode::RateLimited`] with [`EngineError::retry_after_secs`],
/// [`ErrorCode::Network`] / [`ErrorCode::Tls`] (retryable),
/// [`ErrorCode::NotFound`] for a missing message in `fetch_raw`.
pub trait MailApiEngine: Send + Sync + fmt::Debug {
    /// The account's folders (Gmail: the system labels shown as folders).
    fn folders(&self) -> BoxFuture<'_, Result<Vec<RemoteFolder>, EngineError>>;

    /// One page of the full listing; `page` is the previous page's
    /// [`FullSyncPage::next_page`] (`None` = start). `since` limits the
    /// listing to messages newer than this Unix time (sync policy
    /// "recent N days"); `None` = everything.
    fn full_sync_page<'a>(
        &'a self,
        page: Option<&'a str>,
        since: Option<i64>,
    ) -> BoxFuture<'a, Result<FullSyncPage, EngineError>>;

    /// Changes since `cursor`.
    fn changes<'a>(&'a self, cursor: &'a str) -> BoxFuture<'a, Result<DeltaOutcome, EngineError>>;

    /// The raw RFC 5322 bytes of a message.
    fn fetch_raw<'a>(&'a self, remote_id: &'a str) -> BoxFuture<'a, Result<Vec<u8>, EngineError>>;

    /// Replays one queued write. Must be idempotent (a crash replays it
    /// again).
    fn write<'a>(
        &'a self,
        write: &'a RemoteWrite,
    ) -> BoxFuture<'a, Result<WriteOutcome, EngineError>>;
}

/// Adaptive polling for API engines (no push on a desktop client): the
/// interval starts at the minimum, doubles after every quiet poll up to
/// the maximum, and drops back to the minimum after a change or a user
/// action. A server's `Retry-After` overrides it once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptivePoll {
    min_secs: u32,
    max_secs: u32,
    current_secs: u32,
    retry_after_secs: Option<u32>,
}

impl AdaptivePoll {
    /// Polling between `min_secs` (at least 1) and `max_secs` (at least
    /// `min_secs`).
    pub fn new(min_secs: u32, max_secs: u32) -> Self {
        let min_secs = min_secs.max(1);
        Self {
            min_secs,
            max_secs: max_secs.max(min_secs),
            current_secs: min_secs,
            retry_after_secs: None,
        }
    }

    /// Seconds until the next poll.
    pub fn next_delay_secs(&self) -> u32 {
        self.retry_after_secs
            .map_or(self.current_secs, |r| r.max(self.current_secs))
    }

    /// A poll found changes, or the user did something: poll soon again.
    pub fn activity(&mut self) {
        self.current_secs = self.min_secs;
        self.retry_after_secs = None;
    }

    /// A poll found nothing: wait longer next time.
    pub fn quiet(&mut self) {
        self.current_secs = self.current_secs.saturating_mul(2).min(self.max_secs);
        self.retry_after_secs = None;
    }

    /// A poll failed: honour the server's `Retry-After`, else back off
    /// like a quiet poll.
    pub fn failed(&mut self, err: &EngineError) {
        self.quiet();
        self.retry_after_secs = err.retry_after_secs;
    }
}

// ---------------------------------------------------------------------------
// Free/busy (M3b)

/// Answers free/busy questions for attendees (CalDAV schedule outbox, RFC
/// 6638; M3c: Exchange availability). One instance serves one source /
/// account.
pub trait FreeBusyProvider: Send + Sync + fmt::Debug {
    /// Busy periods of each attendee the provider can answer for, in the
    /// query's order; attendees it does not know are left out (the core
    /// reports them as [`crate::scheduling::FreeBusySource::Unknown`]).
    fn free_busy<'a>(
        &'a self,
        query: &'a FreeBusyQuery,
    ) -> BoxFuture<'a, Result<Vec<AttendeeFreeBusy>, EngineError>>;
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

    #[test]
    fn adaptive_poll_backs_off_and_resets() {
        let mut poll = AdaptivePoll::new(30, 300);
        assert_eq!(poll.next_delay_secs(), 30);
        poll.quiet();
        assert_eq!(poll.next_delay_secs(), 60);
        for _ in 0..10 {
            poll.quiet();
        }
        assert_eq!(poll.next_delay_secs(), 300);
        poll.activity();
        assert_eq!(poll.next_delay_secs(), 30);
        poll.failed(&EngineError::rate_limited("quota", 900));
        assert_eq!(poll.next_delay_secs(), 900);
        poll.quiet();
        assert_eq!(poll.next_delay_secs(), 120);
        poll.failed(&EngineError::temporary(ErrorCode::Network, "down"));
        assert_eq!(poll.next_delay_secs(), 240);
        assert_eq!(AdaptivePoll::new(0, 0).next_delay_secs(), 1);
    }

    #[derive(Debug)]
    struct OneMessage;

    impl MailApiEngine for OneMessage {
        fn folders(&self) -> BoxFuture<'_, Result<Vec<RemoteFolder>, EngineError>> {
            Box::pin(async {
                Ok(vec![RemoteFolder {
                    remote_id: "INBOX".into(),
                    name: "Inbox".into(),
                    role: FolderRole::Inbox,
                    parent: None,
                }])
            })
        }

        fn full_sync_page<'a>(
            &'a self,
            page: Option<&'a str>,
            _since: Option<i64>,
        ) -> BoxFuture<'a, Result<FullSyncPage, EngineError>> {
            Box::pin(async move {
                Ok(FullSyncPage {
                    cursor: page.is_none().then(|| "100".to_owned()),
                    messages: vec![],
                    next_page: None,
                })
            })
        }

        fn changes<'a>(
            &'a self,
            cursor: &'a str,
        ) -> BoxFuture<'a, Result<DeltaOutcome, EngineError>> {
            Box::pin(async move {
                Ok(if cursor == "1" {
                    DeltaOutcome::CursorExpired
                } else {
                    DeltaOutcome::Delta(SyncDelta {
                        cursor: "101".into(),
                        removed: vec!["m1".into()],
                        ..SyncDelta::default()
                    })
                })
            })
        }

        fn fetch_raw<'a>(
            &'a self,
            remote_id: &'a str,
        ) -> BoxFuture<'a, Result<Vec<u8>, EngineError>> {
            Box::pin(async move {
                Err(EngineError::permanent(
                    ErrorCode::NotFound,
                    remote_id.to_owned(),
                ))
            })
        }

        fn write<'a>(
            &'a self,
            write: &'a RemoteWrite,
        ) -> BoxFuture<'a, Result<WriteOutcome, EngineError>> {
            Box::pin(async move {
                Ok(match write {
                    RemoteWrite::Append { .. } => WriteOutcome::Done {
                        remote_id: Some("new".into()),
                    },
                    RemoteWrite::Delete { .. } => WriteOutcome::Gone,
                    _ => WriteOutcome::Done { remote_id: None },
                })
            })
        }
    }

    #[test]
    fn mail_api_engine_is_object_safe() {
        let engine: Arc<dyn MailApiEngine> = Arc::new(OneMessage);
        assert_eq!(
            block_on(engine.folders()).unwrap()[0].role,
            FolderRole::Inbox
        );
        let first = block_on(engine.full_sync_page(None, None)).unwrap();
        assert_eq!(first.cursor.as_deref(), Some("100"));
        assert_eq!(
            block_on(engine.changes("1")).unwrap(),
            DeltaOutcome::CursorExpired
        );
        let DeltaOutcome::Delta(delta) = block_on(engine.changes("100")).unwrap() else {
            panic!("expected a delta");
        };
        assert_eq!(delta.removed, ["m1"]);
        assert!(!delta.more);
        let err = block_on(engine.fetch_raw("x")).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let appended = block_on(engine.write(&RemoteWrite::Append {
            folder: "INBOX".into(),
            raw: b"Subject: x\r\n\r\n".to_vec(),
            flags: Flags::default(),
        }))
        .unwrap();
        assert_eq!(
            appended,
            WriteOutcome::Done {
                remote_id: Some("new".into())
            }
        );
        let rate = EngineError::rate_limited("slow down", 60);
        assert!(rate.retryable);
        assert_eq!(rate.code, ErrorCode::RateLimited);
        assert_eq!(rate.retry_after_secs, Some(60));
    }
}
