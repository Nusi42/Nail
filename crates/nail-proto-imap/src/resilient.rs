//! Batch fetches that survive single bad messages (ADR 0191).
//!
//! A sync downloads new mail in batches of up to 50 messages, one `UID
//! FETCH` each. Before, anything wrong with *one* message — the server
//! answering the whole command with `NO` / `BAD` because of it, an answer
//! that cannot be read, a message withheld or too large — failed the batch
//! and with it the folder, at the same place on every retry.
//!
//! [`ImapClient::fetch_batch_resilient`] fetches a batch; when the server
//! refuses it, the batch is split in halves and each half is fetched again,
//! down to single messages. A single message that still fails is reported
//! in [`BatchFetch::failed`] (the caller skips it, records why, and tries it
//! again later) instead of failing the rest. Refusals that concern the
//! mailbox, not a message (`[UNAVAILABLE]`, `[NOPERM]`, …), and errors that
//! break the connection still fail the call.

use crate::bodies::BodyFetch;
use crate::client::{FetchedMessage, ImapClient};
use crate::error::{ImapError, Result};

/// Single messages that may fail in one batch before the problem counts as
/// one of the mailbox and the batch fails as a whole.
const MAX_FAILURES_PER_BATCH: usize = 5;

/// Why a message could not be fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchFailureKind {
    /// The server refused the `UID FETCH` of this message (`NO` / `BAD`).
    Refused,
    /// The server sent no usable answer for it (none, or one Nail cannot
    /// read).
    Withheld,
    /// Its body is larger than Nail reads.
    TooLarge,
}

/// A message [`ImapClient::fetch_batch_resilient`] could not fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchFailure {
    /// The message.
    pub uid: u32,
    /// Why.
    pub kind: FetchFailureKind,
    /// Diagnostic text (English).
    pub detail: String,
    /// The command that failed, as the connection log shows it.
    pub command: Option<String>,
    /// The server's answer, as the connection log shows it.
    pub server_text: Option<String>,
}

/// What [`ImapClient::fetch_batch_resilient`] got.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchFetch {
    /// The messages fetched, in UID order.
    pub messages: Vec<FetchedMessage>,
    /// The messages that could not be fetched.
    pub failed: Vec<FetchFailure>,
}

/// Whether a refusal concerns the mailbox or the session rather than the
/// messages asked for (RFC 5530 codes): splitting the batch would not help.
fn concerns_the_mailbox(err: &ImapError) -> bool {
    let Some(code) = err.response_code() else {
        return false;
    };
    matches!(
        code.to_ascii_uppercase().as_str(),
        "UNAVAILABLE"
            | "INUSE"
            | "LIMIT"
            | "OVERQUOTA"
            | "NOPERM"
            | "AUTHENTICATIONFAILED"
            | "AUTHORIZATIONFAILED"
            | "EXPIRED"
            | "PRIVACYREQUIRED"
            | "CONTACTADMIN"
            | "NONEXISTENT"
    )
}

impl ImapClient {
    /// Fetches `uids` of the examined mailbox under a body policy like
    /// [`Self::fetch_batch_with`], but a message the server refuses,
    /// withholds or sends unreadable or too large is reported in
    /// [`BatchFetch::failed`] instead of failing the batch (see the module
    /// documentation). Errors that break the connection, refusals that
    /// concern the mailbox and more than a handful of failing messages
    /// still fail the call.
    pub async fn fetch_batch_resilient(
        &mut self,
        uids: &[u32],
        bodies: BodyFetch,
    ) -> Result<BatchFetch> {
        let mut out = BatchFetch::default();
        let mut groups: Vec<Vec<u32>> = vec![uids.to_vec()];
        while let Some(group) = groups.pop() {
            if group.is_empty() {
                continue;
            }
            match self.fetch_group(&group, bodies).await {
                Ok(fetched) => {
                    out.messages.extend(fetched.messages);
                    for (uid, kind, detail) in fetched.problems {
                        out.failed.push(self.failure(uid, kind, detail));
                    }
                }
                Err(err @ (ImapError::No(_) | ImapError::Protocol(_) | ImapError::TooLarge(_)))
                    if !self.broken && !concerns_the_mailbox(&err) =>
                {
                    if group.len() == 1 {
                        let kind = if matches!(err, ImapError::TooLarge(_)) {
                            FetchFailureKind::TooLarge
                        } else {
                            FetchFailureKind::Refused
                        };
                        out.failed
                            .push(self.failure(group[0], kind, err.to_string()));
                    } else {
                        tracing::info!(%err, messages = group.len(), "batch refused; splitting it");
                        let (first, second) = group.split_at(group.len() / 2);
                        // Popped in order: first half first.
                        groups.push(second.to_vec());
                        groups.push(first.to_vec());
                    }
                }
                Err(err) => return Err(err),
            }
            if out.failed.len() > MAX_FAILURES_PER_BATCH {
                let last = out
                    .failed
                    .last()
                    .map(|f| f.detail.clone())
                    .unwrap_or_default();
                return Err(ImapError::Protocol(format!(
                    "{} messages of one batch failed; last: {last}",
                    out.failed.len()
                )));
            }
        }
        out.messages.sort_by_key(|m| m.uid);
        out.failed.sort_by_key(|f| f.uid);
        Ok(out)
    }

    fn failure(&self, uid: u32, kind: FetchFailureKind, detail: String) -> FetchFailure {
        FetchFailure {
            uid,
            kind,
            detail,
            command: self.last_command(),
            server_text: self.last_response(),
        }
    }

    /// One group under the body policy; withheld and oversized messages as
    /// problems.
    async fn fetch_group(&mut self, uids: &[u32], bodies: BodyFetch) -> Result<GroupFetch> {
        let mut out = GroupFetch::default();
        let mut parts: Vec<(Vec<u32>, bool)> = Vec::new();
        match bodies {
            BodyFetch::Full => parts.push((uids.to_vec(), false)),
            BodyFetch::HeadersOnly => parts.push((uids.to_vec(), true)),
            BodyFetch::Since(day) => {
                // A server that refuses the date search gets whole messages.
                match self.uids_since(uids, day).await {
                    Ok((recent, older)) => {
                        parts.push((recent, false));
                        parts.push((older, true));
                    }
                    Err(err @ (ImapError::No(_) | ImapError::Protocol(_))) if !self.broken => {
                        tracing::info!(%err, "SEARCH SINCE refused; fetching whole messages");
                        parts.push((uids.to_vec(), false));
                    }
                    Err(err) => return Err(err),
                }
            }
        }
        for (part, headers_only) in parts {
            let fetched = self.fetch_uids_report(&part, headers_only).await?;
            out.messages.extend(fetched.messages);
            for uid in fetched.withheld {
                out.problems.push((
                    uid,
                    FetchFailureKind::Withheld,
                    format!("the server sent no readable answer for message {uid}"),
                ));
            }
            for uid in fetched.too_large {
                out.problems.push((
                    uid,
                    FetchFailureKind::TooLarge,
                    format!("message {uid} is larger than Nail downloads"),
                ));
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Default)]
struct GroupFetch {
    messages: Vec<FetchedMessage>,
    problems: Vec<(u32, FetchFailureKind, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_refusals_are_not_split() {
        assert!(concerns_the_mailbox(&ImapError::No(
            "[UNAVAILABLE] try later".into()
        )));
        assert!(concerns_the_mailbox(&ImapError::No("[NOPERM] no".into())));
        assert!(!concerns_the_mailbox(&ImapError::No(
            "[SERVERBUG] Internal error".into()
        )));
        assert!(!concerns_the_mailbox(&ImapError::No("plain".into())));
        assert!(!concerns_the_mailbox(&ImapError::Protocol("Bad x".into())));
    }
}
