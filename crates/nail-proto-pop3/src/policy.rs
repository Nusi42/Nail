//! Download and leave-on-server planning: pure functions over the server's
//! message list and the client's bookkeeping, so the core can test and
//! replay them without a server.
//!
//! The core keeps, per account, the UIDs it downloaded with the download
//! time, and the UIDs whose local copy the user deleted (emptied from
//! Trash). After [`crate::Pop3Client::messages`]:
//!
//! 1. [`plan_downloads`] → `RETR` these;
//! 2. [`plan_deletions`] → `DELE` these, then `QUIT` to commit;
//! 3. [`vanished`] → forget these UIDs (gone from the server).

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;

use nail_api::accounts::Pop3Options;

use crate::client::ServerMessage;

/// Seconds per day.
const DAY: i64 = 24 * 60 * 60;

/// The effective leave-on-server settings of an account
/// ([`Pop3Options`] with defaults applied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeavePolicy {
    /// Keep messages on the server after downloading them. Default true.
    pub leave_on_server: bool,
    /// With `leave_on_server`: delete this many days after the download.
    pub delete_after_days: Option<u32>,
    /// With `leave_on_server`: delete when the local copy is deleted.
    /// Default false.
    pub delete_when_deleted_locally: bool,
}

impl Default for LeavePolicy {
    fn default() -> Self {
        Self {
            leave_on_server: true,
            delete_after_days: None,
            delete_when_deleted_locally: false,
        }
    }
}

impl From<&Pop3Options> for LeavePolicy {
    fn from(options: &Pop3Options) -> Self {
        let defaults = Self::default();
        Self {
            leave_on_server: options.leave_on_server.unwrap_or(defaults.leave_on_server),
            delete_after_days: options.delete_after_days,
            delete_when_deleted_locally: options
                .delete_when_deleted_locally
                .unwrap_or(defaults.delete_when_deleted_locally),
        }
    }
}

/// Messages to download: those whose UID is not in `known`, in server
/// order, each UID once (some servers list duplicates).
pub fn plan_downloads<'a, S: BuildHasher>(
    server: &'a [ServerMessage],
    known: &HashSet<String, S>,
) -> Vec<&'a ServerMessage> {
    let mut seen = HashSet::new();
    server
        .iter()
        .filter(|m| !known.contains(&m.uid) && seen.insert(m.uid.as_str()))
        .collect()
}

/// Messages to delete from the server under `policy`.
///
/// * `downloaded`: UID → download time (Unix seconds) of every message the
///   client has stored. A message that was never downloaded is never
///   deleted.
/// * `locally_deleted`: UIDs whose local copy was deleted by the user.
/// * `now`: current time (Unix seconds).
///
/// Without leave-on-server every downloaded message is deleted. With it, a
/// message is deleted when it is at least `delete_after_days` old (counted
/// from the download) or, with `delete_when_deleted_locally`, when its local
/// copy is gone.
pub fn plan_deletions<'a, S1: BuildHasher, S2: BuildHasher>(
    policy: &LeavePolicy,
    server: &'a [ServerMessage],
    downloaded: &HashMap<String, i64, S1>,
    locally_deleted: &HashSet<String, S2>,
    now: i64,
) -> Vec<&'a ServerMessage> {
    server
        .iter()
        .filter(|m| {
            let Some(&at) = downloaded.get(&m.uid) else {
                return false;
            };
            if !policy.leave_on_server {
                return true;
            }
            let expired = policy
                .delete_after_days
                .is_some_and(|days| now.saturating_sub(at) >= i64::from(days) * DAY);
            let gone_locally =
                policy.delete_when_deleted_locally && locally_deleted.contains(&m.uid);
            expired || gone_locally
        })
        .collect()
}

/// Known UIDs that are no longer on the server (deleted by another client
/// or by us): their bookkeeping can be dropped.
pub fn vanished<'a>(
    known: impl IntoIterator<Item = &'a String>,
    server: &[ServerMessage],
) -> Vec<String> {
    let present: HashSet<&str> = server.iter().map(|m| m.uid.as_str()).collect();
    let mut out: Vec<String> = known
        .into_iter()
        .filter(|uid| !present.contains(uid.as_str()))
        .cloned()
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs(uids: &[&str]) -> Vec<ServerMessage> {
        uids.iter()
            .enumerate()
            .map(|(i, uid)| ServerMessage {
                number: u32::try_from(i + 1).unwrap(),
                uid: (*uid).to_owned(),
                size: 100,
            })
            .collect()
    }

    fn uids(list: &[&ServerMessage]) -> Vec<String> {
        list.iter().map(|m| m.uid.clone()).collect()
    }

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    use nail_util::time::FIXTURE_LATER as NOW;

    #[test]
    fn options_defaults() {
        assert_eq!(
            LeavePolicy::from(&Pop3Options::default()),
            LeavePolicy::default()
        );
        let policy = LeavePolicy::from(&Pop3Options {
            poll_interval_secs: None,
            leave_on_server: Some(false),
            delete_after_days: Some(7),
            delete_when_deleted_locally: Some(true),
        });
        assert!(!policy.leave_on_server);
        assert_eq!(policy.delete_after_days, Some(7));
        assert!(policy.delete_when_deleted_locally);
    }

    #[test]
    fn downloads_new_uids_once() {
        let server = msgs(&["a", "b", "c", "b"]);
        assert_eq!(uids(&plan_downloads(&server, &set(&["a"]))), ["b", "c"]);
        assert_eq!(plan_downloads(&server, &set(&["a", "b", "c"])).len(), 0);
    }

    #[test]
    fn default_policy_keeps_everything() {
        let server = msgs(&["a", "b"]);
        let downloaded = HashMap::from([("a".to_owned(), 0), ("b".to_owned(), 0)]);
        assert_eq!(
            plan_deletions(
                &LeavePolicy::default(),
                &server,
                &downloaded,
                &set(&["a"]),
                NOW
            )
            .len(),
            0
        );
    }

    #[test]
    fn without_leave_on_server_deletes_downloaded_only() {
        let policy = LeavePolicy {
            leave_on_server: false,
            ..LeavePolicy::default()
        };
        let server = msgs(&["a", "b", "c"]);
        let downloaded = HashMap::from([("a".to_owned(), NOW), ("c".to_owned(), NOW)]);
        assert_eq!(
            uids(&plan_deletions(
                &policy,
                &server,
                &downloaded,
                &HashSet::new(),
                NOW
            )),
            ["a", "c"]
        );
    }

    #[test]
    fn delete_after_days_counts_from_download() {
        let policy = LeavePolicy {
            delete_after_days: Some(7),
            ..LeavePolicy::default()
        };
        let server = msgs(&["old", "edge", "new", "never"]);
        let downloaded = HashMap::from([
            ("old".to_owned(), NOW - 30 * DAY),
            ("edge".to_owned(), NOW - 7 * DAY),
            ("new".to_owned(), NOW - 7 * DAY + 1),
        ]);
        assert_eq!(
            uids(&plan_deletions(
                &policy,
                &server,
                &downloaded,
                &HashSet::new(),
                NOW
            )),
            ["old", "edge"]
        );
        let zero = LeavePolicy {
            delete_after_days: Some(0),
            ..LeavePolicy::default()
        };
        assert_eq!(
            plan_deletions(&zero, &server, &downloaded, &HashSet::new(), NOW).len(),
            3
        );
    }

    #[test]
    fn delete_when_deleted_locally() {
        let policy = LeavePolicy {
            delete_when_deleted_locally: true,
            ..LeavePolicy::default()
        };
        let server = msgs(&["a", "b", "c"]);
        let downloaded = HashMap::from([("a".to_owned(), NOW), ("b".to_owned(), NOW)]);
        // "c" was never downloaded: not deleted even if listed.
        assert_eq!(
            uids(&plan_deletions(
                &policy,
                &server,
                &downloaded,
                &set(&["b", "c"]),
                NOW
            )),
            ["b"]
        );
    }

    #[test]
    fn vanished_uids() {
        let known = [String::from("x"), String::from("a"), String::from("gone")];
        assert_eq!(vanished(&known, &msgs(&["a", "b"])), ["gone", "x"]);
    }
}
