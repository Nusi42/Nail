//! `IDLE` (RFC 2177): wait for the server to report changes to the
//! selected mailbox.

use std::time::Duration;

use imap_proto::{Response, Status};

use crate::changes::MailboxChange;
use crate::client::{ImapClient, response_text};
use crate::conn::Incoming;
use crate::error::{ImapError, Result};
use crate::transport::with_timeout;

/// RFC 2177: clients should re-issue `IDLE` at least every 29 minutes to
/// avoid being logged off by the server's inactivity timer.
pub const IDLE_REISSUE: Duration = Duration::from_mins(29);

/// Why [`ImapClient::idle`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEnd {
    /// The server reported changes (see [`IdleOutcome::changes`]).
    Changes,
    /// The time limit passed without changes: call `idle` again (this is
    /// the periodic re-issue).
    Timeout,
    /// The stop signal fired.
    Stopped,
}

/// Result of [`ImapClient::idle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleOutcome {
    /// Why `IDLE` ended.
    pub end: IdleEnd,
    /// Everything the server reported (may be non-empty for any `end`).
    pub changes: Vec<MailboxChange>,
}

impl ImapClient {
    /// Waits in `IDLE` on the selected mailbox until the server reports a
    /// change, `max` passes (use [`IDLE_REISSUE`] or less) or `stop`
    /// completes; then ends `IDLE` with `DONE`. Changes already collected
    /// return at once without idling.
    ///
    /// The read timeout is suspended while idling (the server may stay
    /// silent for a long time); a dropped connection still ends the wait with
    /// [`ImapError::ConnectionLost`].
    pub async fn idle(
        &mut self,
        max: Duration,
        stop: impl Future<Output = ()> + Send,
    ) -> Result<IdleOutcome> {
        if self.broken {
            return Err(ImapError::ConnectionLost);
        }
        if self.selected.is_none() {
            return Err(ImapError::Protocol(
                "IDLE needs a selected mailbox".to_owned(),
            ));
        }
        if !self.capabilities.has("IDLE") {
            return Err(ImapError::Unsupported("IDLE".to_owned()));
        }
        if !self.changes.is_empty() {
            return Ok(IdleOutcome {
                end: IdleEnd::Changes,
                changes: self.take_changes(),
            });
        }
        let result = self.idle_inner(max, stop).await;
        self.conn.read_timeout().set(Some(self.config.timeout));
        if let Err(err) = &result
            && err.is_transient()
        {
            self.broken = true;
        }
        result
    }

    async fn idle_inner(
        &mut self,
        max: Duration,
        stop: impl Future<Output = ()> + Send,
    ) -> Result<IdleOutcome> {
        let timeout = self.config.timeout;
        let tag = self.conn.next_tag();
        let started = with_timeout(timeout, self.start_idle(&tag)).await?;
        if !started {
            // The server completed IDLE right away (it may not idle now).
            return Ok(IdleOutcome {
                end: IdleEnd::Timeout,
                changes: self.take_changes(),
            });
        }

        self.conn.read_timeout().set(None);
        let deadline = tokio::time::Instant::now() + max;
        tokio::pin!(stop);
        let mut ended_by_server = false;
        let end = loop {
            if !self.changes.is_empty() {
                break IdleEnd::Changes;
            }
            tokio::select! {
                biased;
                () = &mut stop => break IdleEnd::Stopped,
                () = tokio::time::sleep_until(deadline) => break IdleEnd::Timeout,
                incoming = self.conn.read() => match incoming? {
                    Incoming::Response(Response::Done { tag: done, status, code, information })
                        if done.0 == tag =>
                    {
                        if status != Status::Ok {
                            let text = response_text(code.as_ref(), information.as_deref());
                            return Err(ImapError::Protocol(format!("IDLE ended: {text}")));
                        }
                        ended_by_server = true;
                        break IdleEnd::Timeout;
                    }
                    Incoming::Response(response) => self.observe(&response),
                    Incoming::Unparsed => {}
                },
            }
        };
        self.conn.read_timeout().set(Some(timeout));
        if !ended_by_server {
            with_timeout(timeout, async {
                self.conn.write(b"DONE\r\n").await?;
                self.finish(&tag, &mut |_| Ok(false)).await
            })
            .await?;
        }
        Ok(IdleOutcome {
            end,
            changes: self.take_changes(),
        })
    }

    /// Sends `IDLE` and waits for the continuation; `false` when the server
    /// completed the command instead.
    async fn start_idle(&mut self, tag: &str) -> Result<bool> {
        self.conn
            .write(format!("{tag} IDLE\r\n").as_bytes())
            .await?;
        loop {
            match self.conn.read().await? {
                Incoming::Response(Response::Continue { .. }) => return Ok(true),
                Incoming::Response(Response::Done {
                    tag: done,
                    status,
                    code,
                    information,
                }) if done.0 == tag => {
                    let text = response_text(code.as_ref(), information.as_deref());
                    return match status {
                        Status::Ok => Ok(false),
                        Status::No => Err(ImapError::No(text)),
                        other => Err(ImapError::Protocol(format!("{other:?} {text}"))),
                    };
                }
                Incoming::Response(response) => self.observe(&response),
                Incoming::Unparsed => {}
            }
        }
    }
}
