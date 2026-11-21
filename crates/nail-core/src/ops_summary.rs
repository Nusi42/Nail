//! The real state of the queue of message writes (ADR 0230): the summary
//! the status bar shows ([`Request::GetOpsSummary`]), the coalesced
//! [`Event::OpsChanged`] that replaces one event per operation, and the
//! bulk Retry / Dismiss of failed writes.
//!
//! [`Request::GetOpsSummary`]: nail_api::Request::GetOpsSummary

use std::time::{Duration, Instant};

use nail_api::{AccountId, ErrorCode, Event, OpFailure, OpId, OpsSummary};

use crate::error::Result;
use crate::ids;
use crate::service::Service;

/// Shortest time between two [`Event::OpsChanged`] sent while a replay pass
/// runs ([`Service::ops_changed_throttled`]); the pass's end always sends
/// one.
pub(crate) const OPS_CHANGED_INTERVAL: Duration = Duration::from_millis(250);

impl Service {
    /// The queue's state now: counts per account and in total, and the
    /// newest failure.
    pub async fn ops_summary(&self) -> Result<OpsSummary> {
        let (accounts, newest) = self
            .inner
            .store
            .call(|s| Ok((s.write_queue_counts()?, s.newest_failed_op()?)))
            .await?;
        // The class and user-facing text recorded with the failed write
        // (schema v22); writes that failed before it know only the text.
        let newest_failure = newest.map(|op| OpFailure {
            op_id: ids::op_id(op.id),
            account_id: op.account_id,
            code: op.error_code.unwrap_or(ErrorCode::Internal),
            localized: op.error_localized,
            message: op.last_error,
            failed_at: op.updated_at,
        });
        Ok(OpsSummary::from_accounts(accounts, newest_failure))
    }

    /// Sends [`Event::OpsChanged`] with the summary now and returns it.
    /// Calls are serialized, so summaries go out in the order they were
    /// read.
    pub(crate) async fn broadcast_ops_changed(&self) -> Option<OpsSummary> {
        self.send_ops_changed(None).await
    }

    /// [`Self::broadcast_ops_changed`] unless one went out less than
    /// [`OPS_CHANGED_INTERVAL`] ago (a replay pass of single operations).
    pub(crate) async fn ops_changed_throttled(&self) {
        self.send_ops_changed(Some(OPS_CHANGED_INTERVAL)).await;
    }

    async fn send_ops_changed(&self, min_gap: Option<Duration>) -> Option<OpsSummary> {
        let mut last = self.inner.ops_events.lock().await;
        if let (Some(gap), Some(at)) = (min_gap, *last)
            && at.elapsed() < gap
        {
            return None;
        }
        match self.ops_summary().await {
            Ok(summary) => {
                self.broadcast(Event::OpsChanged {
                    summary: summary.clone(),
                });
                *last = Some(Instant::now());
                Some(summary)
            }
            Err(err) => {
                tracing::warn!(%err, "cannot read the operation queue's summary");
                None
            }
        }
    }

    /// [`Self::broadcast_ops_changed`] from synchronous code (a task).
    pub(crate) fn spawn_ops_changed(&self) {
        let service = self.clone();
        tokio::spawn(async move {
            service.broadcast_ops_changed().await;
        });
    }

    /// `RetryFailedOps`: every failed write of `account` (or of all
    /// accounts) back into its queue.
    pub(crate) async fn retry_failed_ops(&self, account: Option<AccountId>) -> Result<Vec<OpId>> {
        let retried = self
            .inner
            .store
            .call(move |s| {
                let retried = s.retry_failed_ops(account.as_ref())?;
                let mut accounts: Vec<AccountId> = Vec::new();
                for id in &retried {
                    let op = s.get_op(*id)?;
                    if !accounts.contains(&op.account_id) {
                        accounts.push(op.account_id);
                    }
                }
                Ok((retried, accounts))
            })
            .await;
        let (retried, accounts) = retried?;
        if !retried.is_empty() {
            self.broadcast_ops_changed().await;
        }
        for account in &accounts {
            self.request_replay(account);
        }
        Ok(retried.into_iter().map(ids::op_id).collect())
    }

    /// `DismissFailedOps`: forgets the failed writes of `account` (or of
    /// all accounts).
    pub(crate) async fn dismiss_failed_ops(&self, account: Option<AccountId>) -> Result<()> {
        let removed = self
            .inner
            .store
            .call(move |s| s.dismiss_failed_ops(account.as_ref()))
            .await?;
        if removed > 0 {
            self.broadcast_ops_changed().await;
        }
        Ok(())
    }
}
