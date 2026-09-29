//! Flag and expunge resynchronization of known messages: QRESYNC
//! (`SELECT ... (QRESYNC ...)` with `VANISHED (EARLIER)`), CONDSTORE
//! (`CHANGEDSINCE` + `UID SEARCH ALL`), or a full `UID FETCH 1:* (FLAGS)`
//! diff on servers without either (RFC 7162).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

use imap_proto::Response;

use crate::changes::{FlagUpdate, flag_update};
use crate::client::{FolderStatus, ImapClient, Part, QresyncParams, uid_set};
use crate::error::{ImapError, Result};
use crate::flags::MessageFlags;

/// Longest known-UID set sent with QRESYNC (it only narrows the server's
/// `VANISHED` answer, so it is optional).
const MAX_KNOWN_UIDS_PARAM: usize = 8 * 1024;

/// What the client knew about a mailbox at its last resync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResyncState {
    /// UIDVALIDITY of the locally stored UIDs.
    pub uid_validity: u32,
    /// `HIGHESTMODSEQ` at the last resync, if the server has mod-sequences.
    /// `None` forces a full flag scan.
    pub highest_modseq: Option<u64>,
}

/// How [`ImapClient::resync_flags`] found the changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResyncMethod {
    /// QRESYNC: changed flags and `VANISHED (EARLIER)` in the `EXAMINE`.
    Qresync,
    /// CONDSTORE: `UID FETCH ... (CHANGEDSINCE ...)` plus `UID SEARCH ALL`
    /// for expunges.
    Condstore,
    /// No usable mod-sequences: all flags fetched and compared.
    FullScan,
}

/// Result of [`ImapClient::resync_flags`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagResync {
    /// Mailbox state at the time of the resync.
    pub status: FolderStatus,
    /// How the changes were found.
    pub method: ResyncMethod,
    /// The server's UIDVALIDITY differs from the known one: every local UID
    /// is invalid; `changed` and `vanished` are empty and the mailbox must be
    /// fetched again (see [`ImapClient::plan_fetch`]).
    pub uid_validity_changed: bool,
    /// Known messages whose flags on the server differ from the local ones.
    pub changed: Vec<FlagUpdate>,
    /// Known UIDs that are no longer on the server.
    pub vanished: Vec<u32>,
    /// The state to store for the next resync.
    pub next_state: ResyncState,
}

impl ImapClient {
    /// Finds which of the locally known messages of `path` changed flags or
    /// disappeared since `known`. `local` maps each known UID to its local
    /// flags; messages the server reports that are not in `local` (new mail)
    /// are ignored, as [`Self::plan_fetch`] handles those.
    ///
    /// Uses QRESYNC when enabled, else CONDSTORE, else a full flag scan;
    /// all three give the same answer.
    pub async fn resync_flags(
        &mut self,
        path: &str,
        known: ResyncState,
        local: &BTreeMap<u32, MessageFlags>,
    ) -> Result<FlagResync> {
        let incremental = known.highest_modseq;
        if let Some(modseq) = incremental
            && self.enabled.qresync
        {
            let uids: Vec<u32> = local.keys().copied().collect();
            let known_uids =
                Some(uid_set(&uids)).filter(|s| !s.is_empty() && s.len() <= MAX_KNOWN_UIDS_PARAM);
            let opened = self
                .open(
                    path,
                    true,
                    Some(QresyncParams {
                        uid_validity: known.uid_validity,
                        modseq,
                        known_uids,
                    }),
                )
                .await?;
            let status = opened.status.ok_or_else(|| no_status(path))?;
            if status.uid_validity != known.uid_validity {
                return Ok(validity_changed(status));
            }
            if let Some(highest) = status.highest_modseq {
                let mut updates: HashMap<u32, FlagUpdate> = HashMap::new();
                for update in opened.fetched {
                    updates.insert(update.uid, update);
                }
                // Changes that raced the EXAMINE arrive as unsolicited data.
                for change in self.take_changes() {
                    if let crate::MailboxChange::Flags {
                        uid: Some(uid),
                        flags,
                        modseq,
                        ..
                    } = change
                    {
                        updates.insert(uid, FlagUpdate { uid, flags, modseq });
                    }
                }
                let vanished = local
                    .keys()
                    .copied()
                    .filter(|uid| in_ranges(&opened.vanished, *uid))
                    .collect();
                return Ok(FlagResync {
                    status,
                    method: ResyncMethod::Qresync,
                    uid_validity_changed: false,
                    changed: differing(updates.into_values(), local),
                    vanished,
                    next_state: ResyncState {
                        uid_validity: status.uid_validity,
                        highest_modseq: Some(highest),
                    },
                });
            }
            return self.full_scan(status, local).await;
        }

        let status = self.examine(path).await?;
        if status.uid_validity != known.uid_validity {
            return Ok(validity_changed(status));
        }
        match (incremental, status.highest_modseq) {
            (Some(since), Some(highest)) => {
                let mut updates = Vec::new();
                if highest > since && status.exists > 0 {
                    updates = self
                        .fetch_flags(&format!("UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE {since})"))
                        .await?;
                }
                let present = if status.exists == 0 {
                    Vec::new()
                } else {
                    self.search("ALL", |_| true).await?
                };
                let vanished = local
                    .keys()
                    .copied()
                    .filter(|uid| present.binary_search(uid).is_err())
                    .collect();
                Ok(FlagResync {
                    status,
                    method: ResyncMethod::Condstore,
                    uid_validity_changed: false,
                    changed: differing(updates, local),
                    vanished,
                    next_state: ResyncState {
                        uid_validity: status.uid_validity,
                        highest_modseq: Some(highest),
                    },
                })
            }
            _ => self.full_scan(status, local).await,
        }
    }

    /// Fetches every message's flags and compares them with `local`.
    async fn full_scan(
        &mut self,
        status: FolderStatus,
        local: &BTreeMap<u32, MessageFlags>,
    ) -> Result<FlagResync> {
        let server = if status.exists == 0 {
            Vec::new()
        } else {
            self.fetch_flags("UID FETCH 1:* (UID FLAGS)").await?
        };
        let present: HashMap<u32, &FlagUpdate> = server.iter().map(|u| (u.uid, u)).collect();
        let vanished = local
            .keys()
            .copied()
            .filter(|uid| !present.contains_key(uid))
            .collect();
        Ok(FlagResync {
            status,
            method: ResyncMethod::FullScan,
            uid_validity_changed: false,
            changed: differing(server.iter().cloned(), local),
            vanished,
            next_state: ResyncState {
                uid_validity: status.uid_validity,
                highest_modseq: status.highest_modseq,
            },
        })
    }

    /// Runs a flag `UID FETCH` (no overall deadline: large mailboxes).
    async fn fetch_flags(&mut self, command: &str) -> Result<Vec<FlagUpdate>> {
        let mut updates = Vec::new();
        let mut on = |response: &Response<'static>| {
            if let Response::Fetch(_, attributes) = response
                && let Some(update) = flag_update(attributes)
            {
                updates.push(update);
                return Ok(true);
            }
            Ok(false)
        };
        self.run_parts(&[Part::Text(Cow::Borrowed(command))], &mut on, false)
            .await?;
        Ok(updates)
    }
}

fn no_status(path: &str) -> ImapError {
    ImapError::Protocol(format!("no status for {path}"))
}

fn validity_changed(status: FolderStatus) -> FlagResync {
    FlagResync {
        status,
        method: ResyncMethod::FullScan,
        uid_validity_changed: true,
        changed: Vec::new(),
        vanished: Vec::new(),
        next_state: ResyncState {
            uid_validity: status.uid_validity,
            highest_modseq: status.highest_modseq,
        },
    }
}

fn in_ranges(ranges: &[RangeInclusive<u32>], uid: u32) -> bool {
    ranges.iter().any(|r| r.contains(&uid))
}

/// The updates for known messages whose flags differ from `local`, by UID.
fn differing(
    updates: impl IntoIterator<Item = FlagUpdate>,
    local: &BTreeMap<u32, MessageFlags>,
) -> Vec<FlagUpdate> {
    let mut out: Vec<FlagUpdate> = updates
        .into_iter()
        .filter(|u| local.get(&u.uid).is_some_and(|l| !l.same_as(&u.flags)))
        .map(|mut u| {
            u.flags = u.flags.normalized();
            u
        })
        .collect();
    out.sort_by_key(|u| u.uid);
    out.dedup_by_key(|u| u.uid);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(atoms: &[&str]) -> MessageFlags {
        MessageFlags::from_imap(atoms.iter().copied())
    }

    #[test]
    fn only_known_and_different_messages_count() {
        let local: BTreeMap<u32, MessageFlags> = [
            (1, flags(&["\\Seen"])),
            (2, flags(&[])),
            (3, flags(&["$A"])),
        ]
        .into();
        let updates = vec![
            FlagUpdate {
                uid: 1,
                flags: flags(&["\\Seen"]),
                modseq: None,
            },
            FlagUpdate {
                uid: 2,
                flags: flags(&["\\Flagged"]),
                modseq: Some(5),
            },
            FlagUpdate {
                uid: 3,
                flags: flags(&["$a"]),
                modseq: None,
            },
            FlagUpdate {
                uid: 9,
                flags: flags(&["\\Seen"]),
                modseq: None,
            },
        ];
        let changed = differing(updates, &local);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].uid, 2);
        assert!(in_ranges(&[1..=3, 8..=8], 8));
        assert!(!in_ranges(&[1..=3], 4));
    }
}
