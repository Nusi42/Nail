//! Confirmations of MCP writes (M4a, ADR 0079): the
//! [`ConfirmationBroker`] trait the core implements, and
//! [`StoreBroker`], the store-backed broker the server uses when the core
//! brings none.
//!
//! A write whose policy asks (always for destructive kinds) becomes a
//! pending [`Confirmation`]: the broker stores it, tells the UIs
//! ([`Event::ConfirmationRequested`]) and lets the tool call wait until the
//! user decided in Nail ([`nail_api::Request::DecideConfirmation`]), the
//! client's elicitation form answered ([`ConfirmationBroker::answer`] with
//! [`ConfirmationSource::Elicitation`]) or the time ran out.
//!
//! [`StoreBroker`] needs nothing but the store: the core's
//! `DecideConfirmation` records the decision there, and a waiting call
//! notices it within [`crate::WriteSettings::poll_interval`] (or at once
//! when someone calls [`StoreBroker::wake`]). It expires overdue
//! confirmations itself, so a call never waits longer than its timeout
//! even before the core's own expiry timer (WP105) runs.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use nail_api::agents::{
    ActionActor, ActionKind, ActionPolicy, Confirmation, ConfirmationDecision, ConfirmationSource,
    MAX_PENDING_PER_ACTOR, NewConfirmation,
};
use nail_api::{ApiError, ConfirmationId, ErrorCode, Event, LocalizedMessage};
use nail_store::Store;

use crate::data::DataResult;
use crate::server::now;

/// A boxed future of a broker call.
pub type BrokerFuture<'a, T> = Pin<Box<dyn Future<Output = DataResult<T>> + Send + 'a>>;

/// What [`ConfirmationBroker::submit`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submitted {
    /// A new pending confirmation (the UIs were told).
    New(Confirmation),
    /// The client used the idempotency key before: that confirmation, in
    /// whatever state it is now.
    Existing(Confirmation),
}

impl Submitted {
    /// The confirmation either way.
    pub fn confirmation(&self) -> &Confirmation {
        match self {
            Self::New(c) | Self::Existing(c) => c,
        }
    }
}

/// Confirmations and policies of MCP writes, as the core provides them.
///
/// Errors are [`ApiError`]s: `rate_limited` when the actor already has
/// [`MAX_PENDING_PER_ACTOR`] pending confirmations, `conflict` when a
/// confirmation is no longer pending.
pub trait ConfirmationBroker: Send + Sync + 'static {
    /// The actor's policy for `kind` (destructive kinds always ask; the
    /// server enforces that on top).
    fn policy(&self, actor: &ActionActor, kind: ActionKind) -> BrokerFuture<'_, ActionPolicy>;

    /// Stores a pending confirmation and tells the UIs.
    fn submit(&self, new: NewConfirmation) -> BrokerFuture<'_, Submitted>;

    /// Waits until the confirmation is decided, expired or cancelled and
    /// returns it in its final state.
    fn wait(&self, id: &ConfirmationId) -> BrokerFuture<'_, Confirmation>;

    /// Records a decision made outside Nail's confirmation center (the MCP
    /// client's elicitation form). Approvals of kinds the source may not
    /// approve ([`ConfirmationSource::may_approve`]) are refused.
    fn answer(
        &self,
        id: &ConfirmationId,
        decision: ConfirmationDecision,
        source: ConfirmationSource,
    ) -> BrokerFuture<'_, Confirmation>;

    /// Records how an approved action ended (`Err` = failed, with the
    /// user-facing reason).
    fn finish(
        &self,
        id: &ConfirmationId,
        outcome: Result<(), LocalizedMessage>,
    ) -> BrokerFuture<'_, Confirmation>;

    /// Cancels a pending confirmation (`None` when it was decided already).
    fn cancel(&self, id: &ConfirmationId) -> BrokerFuture<'_, Option<Confirmation>>;

    /// A confirmation was decided somewhere (the server forwards
    /// [`Event::ConfirmationResolved`]): waiting calls should look now.
    fn wake(&self) {}
}

/// How events reach the UIs.
pub type Emitter = Arc<dyn Fn(Event) + Send + Sync>;

/// The store-backed [`ConfirmationBroker`] (see the module docs).
#[derive(Clone)]
pub struct StoreBroker {
    store: Store,
    emit: Emitter,
    changed: Arc<tokio::sync::Notify>,
    poll: Duration,
}

impl std::fmt::Debug for StoreBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreBroker")
            .field("poll", &self.poll)
            .finish_non_exhaustive()
    }
}

/// When a confirmation expires (Unix seconds), from its RFC 3339 time.
fn expires_at(confirmation: &Confirmation) -> i64 {
    chrono::DateTime::parse_from_rfc3339(&confirmation.expires_at)
        .map(|t| t.timestamp())
        .unwrap_or_default()
}

impl StoreBroker {
    /// A broker over `store`, emitting through `emit`, checking the store
    /// for decisions every `poll`.
    pub fn new(store: Store, emit: Emitter, poll: Duration) -> Self {
        Self {
            store,
            emit,
            changed: Arc::new(tokio::sync::Notify::new()),
            poll: poll.max(Duration::from_millis(10)),
        }
    }

    fn resolved(&self, confirmation: &Confirmation) {
        (self.emit)(Event::ConfirmationResolved {
            confirmation: Box::new(confirmation.clone()),
        });
        self.wake();
    }

    async fn get(&self, id: &ConfirmationId) -> DataResult<Confirmation> {
        let id = id.clone();
        Ok(self.store.call(move |s| s.get_confirmation(&id)).await?)
    }

    async fn wait_final(&self, id: &ConfirmationId) -> DataResult<Confirmation> {
        loop {
            // Register before looking, so a wake between the look and the
            // wait is not lost.
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let current = self.get(id).await?;
            if current.state.is_final() {
                return Ok(current);
            }
            let left = expires_at(&current) - now();
            if left <= 0 {
                let expired = self
                    .store
                    .call(move |s| s.expire_confirmations(now()))
                    .await?;
                for confirmation in &expired {
                    self.resolved(confirmation);
                }
                continue;
            }
            let until_expiry = Duration::from_secs(u64::try_from(left).unwrap_or(1));
            tokio::select! {
                () = &mut notified => {}
                () = tokio::time::sleep(self.poll.min(until_expiry)) => {}
            }
        }
    }
}

impl ConfirmationBroker for StoreBroker {
    fn policy(&self, actor: &ActionActor, kind: ActionKind) -> BrokerFuture<'_, ActionPolicy> {
        let actor = actor.clone();
        Box::pin(async move {
            Ok(self
                .store
                .call(move |s| s.action_policy(&actor, kind))
                .await?)
        })
    }

    fn submit(&self, new: NewConfirmation) -> BrokerFuture<'_, Submitted> {
        Box::pin(async move {
            let inserted = self
                .store
                .call(move |s| s.insert_confirmation(new, now(), MAX_PENDING_PER_ACTOR))
                .await
                .map_err(|err| match err {
                    // The only conflict of an insert: the pending cap.
                    nail_store::Error::Conflict(detail) => {
                        ApiError::new(ErrorCode::RateLimited, detail)
                    }
                    other => ApiError::from(other),
                })?;
            Ok(match inserted {
                nail_store::InsertedConfirmation::New(confirmation) => {
                    (self.emit)(Event::ConfirmationRequested {
                        confirmation: Box::new(confirmation.clone()),
                    });
                    Submitted::New(confirmation)
                }
                nail_store::InsertedConfirmation::Existing(confirmation) => {
                    Submitted::Existing(confirmation)
                }
            })
        })
    }

    fn wait(&self, id: &ConfirmationId) -> BrokerFuture<'_, Confirmation> {
        let id = id.clone();
        Box::pin(async move { self.wait_final(&id).await })
    }

    fn answer(
        &self,
        id: &ConfirmationId,
        decision: ConfirmationDecision,
        source: ConfirmationSource,
    ) -> BrokerFuture<'_, Confirmation> {
        let id = id.clone();
        Box::pin(async move {
            let approve = decision == ConfirmationDecision::Approve;
            let decided = self
                .store
                .call(move |s| s.decide_confirmation(&id, approve, source, now()))
                .await?;
            self.resolved(&decided);
            Ok(decided)
        })
    }

    fn finish(
        &self,
        id: &ConfirmationId,
        outcome: Result<(), LocalizedMessage>,
    ) -> BrokerFuture<'_, Confirmation> {
        let id = id.clone();
        Box::pin(async move {
            let failed = outcome.is_err();
            let finished = self
                .store
                .call(move |s| s.finish_confirmation(&id, outcome))
                .await?;
            if failed {
                self.resolved(&finished);
            }
            Ok(finished)
        })
    }

    fn cancel(&self, id: &ConfirmationId) -> BrokerFuture<'_, Option<Confirmation>> {
        let id = id.clone();
        Box::pin(async move {
            let cancelled = self
                .store
                .call(move |s| s.cancel_confirmation(&id, now()))
                .await?;
            if let Some(confirmation) = &cancelled {
                self.resolved(confirmation);
            }
            Ok(cancelled)
        })
    }

    /// Wakes every waiting call to look at the store now.
    fn wake(&self) {
        self.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::agents::{ActionAttribution, ActionTarget, ActionTargetKind, ConfirmationState};
    use std::sync::Mutex;

    fn new(key: Option<&str>, timeout_secs: u64) -> NewConfirmation {
        NewConfirmation {
            requested_by: ActionAttribution {
                actor: ActionActor::mcp_client("1"),
                name: "Claude".into(),
                on_behalf_of: None,
            },
            kind: ActionKind::Send,
            operation: "send_message".into(),
            summary: LocalizedMessage::new("confirmation-summary-send")
                .arg("actor", "Claude")
                .arg("subject", "Hi")
                .arg("count", 1),
            targets: vec![ActionTarget {
                kind: ActionTargetKind::Draft,
                id: "d1".into(),
                label: "Hi".into(),
            }],
            changes: vec![],
            account_ids: vec!["work".into()],
            payload: serde_json::json!({"draft_id": "d1"}),
            idempotency_key: key.map(str::to_owned),
            timeout_secs,
        }
    }

    fn broker() -> (StoreBroker, Arc<Mutex<Vec<Event>>>) {
        let store = Store::open_in_memory().unwrap();
        // Confirmations name their MCP client.
        store
            .create_mcp_client(
                "Claude",
                "hash",
                "hint",
                &nail_api::mcp::McpGrant::default(),
                0,
            )
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let broker = StoreBroker::new(
            store,
            Arc::new(move |e| sink.lock().unwrap().push(e)),
            Duration::from_millis(20),
        );
        (broker, events)
    }

    #[tokio::test]
    async fn submit_wait_and_decide() {
        let (broker, events) = broker();
        let submitted = broker.submit(new(Some("k"), 60)).await.unwrap();
        let Submitted::New(c) = &submitted else {
            panic!("new expected")
        };
        assert_eq!(c.state, ConfirmationState::Pending);
        assert!(matches!(
            events.lock().unwrap()[0],
            Event::ConfirmationRequested { .. }
        ));
        // The same key returns the same confirmation.
        let again = broker.submit(new(Some("k"), 60)).await.unwrap();
        assert!(matches!(again, Submitted::Existing(ref e) if e.id == c.id));

        let waiter = {
            let broker = broker.clone();
            let id = c.id.clone();
            tokio::spawn(async move { broker.wait(&id).await })
        };
        // A decision written to the store by someone else (the core) is
        // noticed by polling.
        let id = c.id.clone();
        broker
            .store
            .call(move |s| s.decide_confirmation(&id, false, ConfirmationSource::Nail, now()))
            .await
            .unwrap();
        let decided = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(decided.state, ConfirmationState::Denied);
        // A decided confirmation cannot be answered again.
        let err = broker
            .answer(
                &c.id,
                ConfirmationDecision::Approve,
                ConfirmationSource::Elicitation,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
    }

    #[tokio::test]
    async fn elicitation_cannot_approve_destructive_kinds_and_expiry_ends_the_wait() {
        let (broker, events) = broker();
        let mut trash = new(None, 1);
        trash.kind = ActionKind::Trash;
        let c = broker.submit(trash).await.unwrap().confirmation().clone();
        let err = broker
            .answer(
                &c.id,
                ConfirmationDecision::Approve,
                ConfirmationSource::Elicitation,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        let started = std::time::Instant::now();
        let expired = broker.wait(&c.id).await.unwrap();
        assert_eq!(expired.state, ConfirmationState::Expired);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, Event::ConfirmationResolved { .. }))
        );
    }

    #[tokio::test]
    async fn the_pending_cap_is_a_rate_limit_and_finish_records_failures() {
        let (broker, _) = broker();
        for _ in 0..MAX_PENDING_PER_ACTOR {
            broker.submit(new(None, 60)).await.unwrap();
        }
        let err = broker.submit(new(None, 60)).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
        assert!(
            broker
                .policy(&ActionActor::mcp_client("1"), ActionKind::Send)
                .await
                .unwrap()
                .asks()
        );

        let c = broker
            .store
            .list_confirmations(
                nail_api::agents::ConfirmationFilter::default(),
                None,
                None,
                1,
            )
            .unwrap()
            .remove(0);
        let approved = broker
            .answer(
                &c.id,
                ConfirmationDecision::Approve,
                ConfirmationSource::Nail,
            )
            .await
            .unwrap();
        assert_eq!(approved.state, ConfirmationState::Approved);
        let failed = broker
            .finish(&c.id, Err(LocalizedMessage::new("error-network")))
            .await
            .unwrap();
        assert_eq!(failed.state, ConfirmationState::Failed);
        let other = broker
            .store
            .list_confirmations(
                nail_api::agents::ConfirmationFilter::default(),
                None,
                None,
                1,
            )
            .unwrap()
            .remove(0);
        assert!(broker.cancel(&other.id).await.unwrap().is_some());
        assert!(broker.cancel(&other.id).await.unwrap().is_none());
    }
}
