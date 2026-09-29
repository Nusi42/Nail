//! Mailbox changes the server reports on its own: new messages, expunges
//! and flag changes made by other clients (RFC 3501 section 7, RFC 7162
//! `VANISHED`). They arrive during any command, `NOOP` or `IDLE`; the client
//! collects them until [`ImapClient::take_changes`].

use imap_proto::Response;
use imap_proto::types::{AttributeValue, MailboxDatum};

use crate::client::ImapClient;
use crate::error::Result;
use crate::flags::MessageFlags;

/// Upper bound for UIDs expanded from one `VANISHED` response.
const MAX_VANISHED: usize = 1_000_000;

/// A change to the selected mailbox that the server reported.
///
/// These are hints: the reliable way to apply them is an incremental sync
/// ([`ImapClient::plan_fetch`] for new mail, [`ImapClient::resync_flags`]
/// for flags and expunges). They tell a watcher *when* to sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailboxChange {
    /// The mailbox now has this many messages (usually: new mail).
    Exists(u32),
    /// The message with this sequence number was expunged (servers without
    /// QRESYNC). Later sequence numbers shift down by one.
    Expunged(u32),
    /// Messages with these UIDs were expunged (QRESYNC `VANISHED`).
    Vanished(Vec<u32>),
    /// A message's flags changed.
    Flags {
        /// Sequence number.
        seq: u32,
        /// UID, when the server included it (always with QRESYNC).
        uid: Option<u32>,
        /// The new flags.
        flags: MessageFlags,
        /// The new mod-sequence (CONDSTORE).
        modseq: Option<u64>,
    },
}

/// The flags of one message as a `FETCH` response reported them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagUpdate {
    /// The message UID.
    pub uid: u32,
    /// Its flags.
    pub flags: MessageFlags,
    /// Its mod-sequence (CONDSTORE).
    pub modseq: Option<u64>,
}

/// UID, flags and mod-sequence from `FETCH` attributes; `None` without UID
/// or FLAGS.
pub(crate) fn flag_update(attributes: &[AttributeValue<'_>]) -> Option<FlagUpdate> {
    let (uid, flags, modseq) = fetch_flags(attributes);
    Some(FlagUpdate {
        uid: uid?,
        flags: flags?,
        modseq,
    })
}

fn fetch_flags(
    attributes: &[AttributeValue<'_>],
) -> (Option<u32>, Option<MessageFlags>, Option<u64>) {
    let (mut uid, mut flags, mut modseq) = (None, None, None);
    for attribute in attributes {
        match attribute {
            AttributeValue::Uid(value) => uid = Some(*value),
            AttributeValue::Flags(values) => {
                flags = Some(MessageFlags::from_imap(
                    values.iter().map(AsRef::<str>::as_ref),
                ));
            }
            AttributeValue::ModSeq(value) => modseq = Some(*value),
            _ => {}
        }
    }
    (uid, flags, modseq)
}

/// Expands UID ranges (at most [`MAX_VANISHED`] UIDs).
pub(crate) fn expand_ranges(ranges: &[std::ops::RangeInclusive<u32>]) -> Vec<u32> {
    let mut out = Vec::new();
    for range in ranges {
        for uid in range.clone() {
            if out.len() >= MAX_VANISHED {
                tracing::warn!("VANISHED response truncated");
                return out;
            }
            out.push(uid);
        }
    }
    out
}

impl ImapClient {
    /// Records an unsolicited response as a change of the selected mailbox.
    pub(crate) fn observe(&mut self, response: &Response<'static>) {
        let Some(selected) = self.selected.as_mut() else {
            return;
        };
        let change = match response {
            Response::MailboxData(MailboxDatum::Exists(n)) => {
                selected.status.exists = *n;
                MailboxChange::Exists(*n)
            }
            Response::Expunge(seq) => {
                selected.status.exists = selected.status.exists.saturating_sub(1);
                MailboxChange::Expunged(*seq)
            }
            Response::Vanished {
                earlier: false,
                uids,
            } => {
                let uids = expand_ranges(uids);
                let count = u32::try_from(uids.len()).unwrap_or(u32::MAX);
                selected.status.exists = selected.status.exists.saturating_sub(count);
                MailboxChange::Vanished(uids)
            }
            Response::Fetch(seq, attributes) => {
                let (uid, flags, modseq) = fetch_flags(attributes);
                let Some(flags) = flags else {
                    return;
                };
                MailboxChange::Flags {
                    seq: *seq,
                    uid,
                    flags,
                    modseq,
                }
            }
            _ => return,
        };
        self.changes.push(change);
    }

    /// Takes the changes the server reported since the last call (during
    /// any command, [`Self::noop`] or [`Self::idle`]). Selecting another
    /// mailbox discards them.
    pub fn take_changes(&mut self) -> Vec<MailboxChange> {
        std::mem::take(&mut self.changes)
    }

    /// Changes collected so far, without taking them.
    pub fn pending_changes(&self) -> &[MailboxChange] {
        &self.changes
    }

    /// Polls for changes: `NOOP`, then [`Self::take_changes`]. The fallback
    /// for servers without `IDLE`.
    pub async fn poll(&mut self) -> Result<Vec<MailboxChange>> {
        self.noop().await?;
        Ok(self.take_changes())
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    #[test]
    fn flag_updates_need_uid_and_flags() {
        let attributes = [
            AttributeValue::Uid(3),
            AttributeValue::Flags(vec![Cow::Borrowed("\\Flagged")]),
            AttributeValue::ModSeq(9),
        ];
        let update = flag_update(&attributes).unwrap();
        assert_eq!(update.uid, 3);
        assert!(update.flags.flags.flagged);
        assert_eq!(update.modseq, Some(9));
        assert!(flag_update(&attributes[1..]).is_none());
        assert!(flag_update(&attributes[..1]).is_none());
    }

    #[test]
    fn ranges_expand() {
        assert_eq!(expand_ranges(&[1..=3, 7..=7]), [1, 2, 3, 7]);
        assert_eq!(expand_ranges(&[]), Vec::<u32>::new());
        assert_eq!(expand_ranges(&[1..=u32::MAX]).len(), MAX_VANISHED);
    }
}
