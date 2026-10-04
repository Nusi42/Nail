//! [`MailboxWatcher`]: keeps a dedicated connection on one mailbox and
//! reports server-side changes as a stream of [`WatchEvent`]s. Uses `IDLE`
//! (re-issued every 29 minutes) or, on servers without it, `NOOP` polling;
//! reconnects with exponential backoff after errors.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use futures::future::BoxFuture;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::changes::MailboxChange;
use crate::client::{FolderStatus, ImapClient};
use crate::config::{Credentials, ImapConfig};
use crate::error::{ImapError, Result};
use crate::idle::{IDLE_REISSUE, IdleEnd};

/// Supplies credentials for each (re)connection. OAuth2 access tokens
/// expire, so the watcher asks again for every connection, with `refresh`
/// set after the server rejected the previous ones.
pub trait CredentialProvider: Send + Sync {
    /// Credentials for the next login.
    fn credentials(&self, refresh: bool) -> BoxFuture<'_, Result<Credentials>>;
}

/// Fixed credentials (passwords, or a token in tests).
impl CredentialProvider for Credentials {
    fn credentials(&self, _refresh: bool) -> BoxFuture<'_, Result<Credentials>> {
        let credentials = self.clone();
        Box::pin(async move { Ok(credentials) })
    }
}

/// What a [`MailboxWatcher`] watches and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchConfig {
    /// Server path of the mailbox (default `INBOX`).
    pub mailbox: String,
    /// Use `IDLE` when the server offers it (default `true`); otherwise
    /// poll with `NOOP`.
    pub use_idle: bool,
    /// Re-issue `IDLE` after this long (default 29 minutes, RFC 2177).
    pub idle_reissue: Duration,
    /// Interval of `NOOP` polling (default 2 minutes).
    pub poll_interval: Duration,
    /// First reconnect delay (default 1 second); doubles after each failed
    /// attempt up to `reconnect_max`.
    pub reconnect_min: Duration,
    /// Longest reconnect delay (default 5 minutes).
    pub reconnect_max: Duration,
    /// A connection that lasted this long counts as healthy: the backoff
    /// starts again at `reconnect_min` after it fails (default 1 minute).
    /// A connection that fails sooner (a server that accepts the login but
    /// drops `IDLE`) keeps the backoff growing, so it is not hammered.
    pub healthy_after: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self::new("INBOX")
    }
}

impl WatchConfig {
    /// Watches `mailbox` with the defaults.
    pub fn new(mailbox: impl Into<String>) -> Self {
        Self {
            mailbox: mailbox.into(),
            use_idle: true,
            idle_reissue: IDLE_REISSUE,
            poll_interval: Duration::from_mins(2),
            reconnect_min: Duration::from_secs(1),
            reconnect_max: Duration::from_mins(5),
            healthy_after: Duration::from_mins(1),
        }
    }

    /// Enables or disables `IDLE` (polling only when disabled).
    #[must_use]
    pub fn with_idle(mut self, use_idle: bool) -> Self {
        self.use_idle = use_idle;
        self
    }

    /// Sets the `IDLE` re-issue interval.
    #[must_use]
    pub fn with_idle_reissue(mut self, interval: Duration) -> Self {
        self.idle_reissue = interval;
        self
    }

    /// Sets the polling interval.
    #[must_use]
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Sets how long a connection must last to reset the backoff.
    #[must_use]
    pub fn with_healthy_after(mut self, after: Duration) -> Self {
        self.healthy_after = after;
        self
    }

    /// Sets the reconnect backoff bounds.
    #[must_use]
    pub fn with_reconnect(mut self, min: Duration, max: Duration) -> Self {
        self.reconnect_min = min;
        self.reconnect_max = max.max(min);
        self
    }
}

/// How the watcher learns about changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchMode {
    /// `IDLE`: the server pushes changes.
    Idle,
    /// `NOOP` every [`WatchConfig::poll_interval`].
    Poll,
}

/// An event of a [`MailboxWatcher`].
#[derive(Debug)]
pub enum WatchEvent {
    /// Connected and watching. Anything may have changed while the watcher
    /// was not connected, so this is the moment for a full incremental sync.
    Connected {
        /// IDLE or polling.
        mode: WatchMode,
        /// The mailbox state after `EXAMINE`.
        status: FolderStatus,
    },
    /// The server reported changes; sync the mailbox.
    Changed(Vec<MailboxChange>),
    /// The connection failed or could not be established; the watcher tries
    /// again after `retry_in`.
    Disconnected {
        /// What went wrong.
        error: ImapError,
        /// Delay before the next attempt.
        retry_in: Duration,
    },
}

/// Watches one mailbox on its own connection; see the module docs.
/// Dropping it stops the watcher.
#[derive(Debug)]
pub struct MailboxWatcher {
    events: mpsc::Receiver<WatchEvent>,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl MailboxWatcher {
    /// Starts watching on the current tokio runtime.
    pub fn spawn(
        config: ImapConfig,
        credentials: Arc<dyn CredentialProvider>,
        watch: WatchConfig,
    ) -> Self {
        let (events_tx, events) = mpsc::channel(64);
        let (stop, stop_rx) = watch::channel(false);
        let task = tokio::spawn(run(config, credentials, watch, events_tx, stop_rx));
        Self {
            events,
            stop,
            task: Some(task),
        }
    }

    /// The next event; `None` once the watcher has stopped.
    pub async fn next_event(&mut self) -> Option<WatchEvent> {
        self.events.recv().await
    }

    /// Stops watching: ends `IDLE`, logs out and waits for the task.
    pub async fn stop(mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            drop(task.await);
        }
    }
}

impl Stream for MailboxWatcher {
    type Item = WatchEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<WatchEvent>> {
        self.get_mut().events.poll_recv(cx)
    }
}

impl Drop for MailboxWatcher {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

/// Completes when a stop is requested (or the watcher handle is gone).
async fn stopped(stop: &mut watch::Receiver<bool>) {
    drop(stop.wait_for(|stopped| *stopped).await);
}

async fn run(
    config: ImapConfig,
    credentials: Arc<dyn CredentialProvider>,
    watch: WatchConfig,
    events: mpsc::Sender<WatchEvent>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = watch.reconnect_min;
    let mut refresh = false;
    loop {
        if *stop.borrow() {
            return;
        }
        let connect = async {
            let credentials = credentials.credentials(refresh).await?;
            let mut client = ImapClient::connect(&config, &credentials).await?;
            let status = client.examine(&watch.mailbox).await?;
            Ok::<_, ImapError>((client, status))
        };
        let connected = tokio::select! {
            result = connect => result,
            () = stopped(&mut stop) => return,
        };
        let error = match connected {
            Ok((mut client, status)) => {
                refresh = false;
                let since = tokio::time::Instant::now();
                let mode = if watch.use_idle && client.capabilities().has("IDLE") {
                    WatchMode::Idle
                } else {
                    WatchMode::Poll
                };
                tracing::debug!(mailbox = %watch.mailbox, ?mode, "watching");
                if events
                    .send(WatchEvent::Connected { mode, status })
                    .await
                    .is_err()
                {
                    return;
                }
                match Box::pin(serve(&mut client, mode, &watch, &events, &mut stop)).await {
                    Ok(()) => {
                        drop(tokio::time::timeout(config.timeout, client.logout()).await);
                        return;
                    }
                    Err(err) => {
                        // Only a connection that stayed up for a while
                        // resets the backoff.
                        if since.elapsed() >= watch.healthy_after {
                            backoff = watch.reconnect_min;
                        }
                        err
                    }
                }
            }
            Err(err) => {
                refresh = matches!(err, ImapError::AuthFailed(_));
                err
            }
        };
        tracing::debug!(%error, retry_in = ?backoff, "mailbox watcher disconnected");
        let retry_in = backoff;
        if events
            .send(WatchEvent::Disconnected { error, retry_in })
            .await
            .is_err()
        {
            return;
        }
        tokio::select! {
            () = tokio::time::sleep(retry_in) => {}
            () = stopped(&mut stop) => return,
        }
        backoff = (backoff * 2).min(watch.reconnect_max);
    }
}

/// Watches until stopped (`Ok`) or the connection fails (`Err`).
async fn serve(
    client: &mut ImapClient,
    mode: WatchMode,
    watch: &WatchConfig,
    events: &mpsc::Sender<WatchEvent>,
    stop: &mut watch::Receiver<bool>,
) -> Result<()> {
    loop {
        let changes = match mode {
            WatchMode::Idle => {
                let outcome = client.idle(watch.idle_reissue, stopped(stop)).await?;
                if outcome.end == IdleEnd::Stopped {
                    return Ok(());
                }
                outcome.changes
            }
            WatchMode::Poll => {
                tokio::select! {
                    () = tokio::time::sleep(watch.poll_interval) => {}
                    () = stopped(stop) => return Ok(()),
                }
                client.poll().await?
            }
        };
        if !changes.is_empty() && events.send(WatchEvent::Changed(changes)).await.is_err() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_builders() {
        let config = WatchConfig::default();
        assert_eq!(config.mailbox, "INBOX");
        assert!(config.use_idle);
        assert_eq!(config.idle_reissue, Duration::from_mins(29));
        let config = WatchConfig::new("Work")
            .with_idle(false)
            .with_poll_interval(Duration::from_secs(5))
            .with_idle_reissue(Duration::from_mins(1))
            .with_reconnect(Duration::from_secs(10), Duration::from_secs(1));
        assert!(!config.use_idle);
        assert_eq!(config.reconnect_max, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn static_credentials_are_a_provider() {
        let provider: Arc<dyn CredentialProvider> = Arc::new(Credentials::new("sam", "pw"));
        let credentials = provider.credentials(true).await.unwrap();
        assert_eq!(credentials.username, "sam");
    }
}
