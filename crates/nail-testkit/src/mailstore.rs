//! The in-memory mailbox state served by the mock IMAP server.
//!
//! [`MailStore`] is cheaply cloneable and shared between the server and the
//! test, so tests can add and remove messages, change flags or reset
//! UIDVALIDITY while clients are connected. Every change bumps the mailbox's
//! mod-sequence (RFC 7162) and wakes sessions waiting in `IDLE`, which then
//! push `EXISTS` / `EXPUNGE` / `VANISHED` / `FETCH` updates like a real
//! server does when another client changes the mailbox.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use mail_parser::MessageParser;
use nail_api::FolderRole;
use tokio::sync::watch;

use crate::corpus::{CorpusFolder, demo_corpus, to_crlf};

/// Default internal date for messages without a usable `Date` header.
const FALLBACK_INTERNAL_DATE: &str = "01-Jan-2026 00:00:00 +0000";

/// One stored message.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    /// The message UID (unique within the mailbox's UIDVALIDITY epoch).
    pub uid: u32,
    /// IMAP flags as atoms, e.g. `\Seen` or a keyword such as `$Label1`.
    pub flags: Vec<String>,
    /// IMAP `INTERNALDATE`, e.g. `28-Sep-2026 08:15:00 +0200`.
    pub internal_date: String,
    /// Raw RFC 5322 bytes (CRLF).
    pub raw: Arc<Vec<u8>>,
    /// Mod-sequence of the last change to this message (RFC 7162).
    pub modseq: u64,
}

impl StoredMessage {
    /// Whether the message carries `flag` (case-insensitive).
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f.eq_ignore_ascii_case(flag))
    }
}

/// One mailbox.
#[derive(Debug, Clone)]
pub struct Mailbox {
    /// Full mailbox name, hierarchy delimiter `/`.
    pub name: String,
    /// Special-use role, advertised in `LIST` responses.
    pub role: FolderRole,
    /// Current UIDVALIDITY.
    pub uid_validity: u32,
    /// Next UID to assign.
    pub uid_next: u32,
    /// Messages in UID order.
    pub messages: Vec<StoredMessage>,
    /// Highest mod-sequence of the mailbox (RFC 7162 `HIGHESTMODSEQ`).
    pub highest_modseq: u64,
    /// Expunged UIDs with the mod-sequence of their removal, for
    /// `VANISHED (EARLIER)` answers (RFC 7162 QRESYNC).
    pub expunged: Vec<(u32, u64)>,
}

impl Mailbox {
    fn bump(&mut self) -> u64 {
        self.highest_modseq += 1;
        self.highest_modseq
    }

    /// The message with `uid`.
    pub fn message(&self, uid: u32) -> Option<&StoredMessage> {
        self.messages
            .binary_search_by_key(&uid, |m| m.uid)
            .ok()
            .map(|i| &self.messages[i])
    }
}

/// How [`MailStore::store_flags`] changes the flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagUpdate {
    /// `+FLAGS`: add.
    Add,
    /// `-FLAGS`: remove.
    Remove,
    /// `FLAGS`: replace.
    Replace,
}

/// A message's flags after [`MailStore::store_flags`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFlags {
    /// The message UID.
    pub uid: u32,
    /// Flags after the update.
    pub flags: Vec<String>,
    /// Mod-sequence after the update.
    pub modseq: u64,
}

/// Result of [`MailStore::store_flags`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreOutcome {
    /// Every addressed message that exists and was not refused.
    pub updated: Vec<StoredFlags>,
    /// UIDs refused by `UNCHANGEDSINCE` (changed since).
    pub refused: Vec<u32>,
}

/// Why a mailbox operation failed (mirrors IMAP response codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MailboxError {
    /// The mailbox does not exist (`NONEXISTENT`).
    #[error("no such mailbox")]
    NonExistent,
    /// The target name is taken (`ALREADYEXISTS`).
    #[error("mailbox already exists")]
    AlreadyExists,
    /// The operation is not allowed on this mailbox (e.g. renaming `INBOX`).
    #[error("not allowed for this mailbox")]
    NotAllowed,
}

#[derive(Debug)]
struct State {
    mailboxes: Vec<Mailbox>,
    next_uid_validity: u32,
}

/// Shared mailbox state of a mock server.
#[derive(Debug, Clone)]
pub struct MailStore {
    state: Arc<Mutex<State>>,
    /// Bumped on every change; sessions in `IDLE` wait on it.
    version: Arc<watch::Sender<u64>>,
}

impl Default for MailStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MailStore {
    /// An empty store with an `INBOX`.
    pub fn new() -> Self {
        let store = Self {
            state: Arc::new(Mutex::new(State {
                mailboxes: Vec::new(),
                next_uid_validity: 1_700_000_001,
            })),
            version: Arc::new(watch::channel(0).0),
        };
        store.add_mailbox("INBOX", FolderRole::Inbox);
        store
    }

    /// A store filled from corpus folders (UIDs 1.. in folder order).
    pub fn from_folders(folders: impl IntoIterator<Item = CorpusFolder>) -> Self {
        let store = Self::new();
        for folder in folders {
            store.add_mailbox(&folder.name, folder.role);
            for message in folder.messages {
                store.append(&folder.name, &message.raw, &message.flags);
            }
        }
        store
    }

    /// A store filled with the embedded demo corpus.
    pub fn demo() -> Self {
        Self::from_folders(demo_corpus())
    }

    /// A store loaded from a directory (see [`crate::load_mail_dir`]).
    pub fn from_dir(dir: &std::path::Path) -> std::io::Result<Self> {
        Ok(Self::from_folders(crate::corpus::load_mail_dir(dir)?))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panicking test thread must not wedge the server for other tests.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs `f` on a mailbox under the lock and wakes waiting sessions
    /// afterwards.
    fn change<R>(&self, name: &str, f: impl FnOnce(&mut Mailbox) -> R) -> Option<R> {
        let result = {
            let mut state = self.lock();
            let index = find_index(&state.mailboxes, name)?;
            f(&mut state.mailboxes[index])
        };
        self.notify();
        Some(result)
    }

    fn notify(&self) {
        self.version.send_modify(|v| *v += 1);
    }

    /// A receiver that changes whenever any mailbox changes.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.version.subscribe()
    }

    /// Creates a mailbox if it does not exist yet (`INBOX` is matched
    /// case-insensitively). Returns `false` when it already existed.
    pub fn add_mailbox(&self, name: &str, role: FolderRole) -> bool {
        {
            let mut state = self.lock();
            if find_index(&state.mailboxes, name).is_some() {
                return false;
            }
            let uid_validity = state.next_uid_validity;
            state.next_uid_validity += 1;
            let name = if name.eq_ignore_ascii_case("INBOX") {
                "INBOX"
            } else {
                name
            };
            state.mailboxes.push(Mailbox {
                name: name.to_owned(),
                role,
                uid_validity,
                uid_next: 1,
                messages: Vec::new(),
                highest_modseq: 1,
                expunged: Vec::new(),
            });
        }
        self.notify();
        true
    }

    /// Appends a message and returns its UID, or `None` when the mailbox does
    /// not exist. Line endings are normalized to CRLF.
    pub fn append<S: AsRef<str>>(&self, mailbox: &str, raw: &[u8], flags: &[S]) -> Option<u32> {
        self.append_with_date(mailbox, raw, flags, None)
    }

    /// Adds a message (alias of [`Self::append`], for tests that read better
    /// with "add" / "remove").
    pub fn add_message<S: AsRef<str>>(
        &self,
        mailbox: &str,
        raw: &[u8],
        flags: &[S],
    ) -> Option<u32> {
        self.append(mailbox, raw, flags)
    }

    /// Like [`Self::append`] with an explicit `INTERNALDATE`
    /// (`28-Sep-2026 08:15:00 +0200`); `None` derives it from the `Date`
    /// header.
    pub fn append_with_date<S: AsRef<str>>(
        &self,
        mailbox: &str,
        raw: &[u8],
        flags: &[S],
        internal_date: Option<&str>,
    ) -> Option<u32> {
        let raw = to_crlf(raw);
        let internal_date = internal_date.map_or_else(|| internal_date_of(&raw), str::to_owned);
        let flags = normalize_flags(flags.iter().map(AsRef::as_ref));
        self.change(mailbox, |mailbox| {
            let uid = mailbox.uid_next;
            mailbox.uid_next += 1;
            let modseq = mailbox.bump();
            mailbox.messages.push(StoredMessage {
                uid,
                flags,
                internal_date,
                raw: Arc::new(raw),
                modseq,
            });
            uid
        })
    }

    /// Simulates a server-side mailbox rebuild: assigns a new UIDVALIDITY and
    /// renumbers all messages from UID 1. Returns the new UIDVALIDITY.
    pub fn reset_uid_validity(&self, mailbox: &str) -> Option<u32> {
        let uid_validity = {
            let mut state = self.lock();
            let index = find_index(&state.mailboxes, mailbox)?;
            let uid_validity = state.next_uid_validity;
            state.next_uid_validity += 1;
            let mailbox = &mut state.mailboxes[index];
            mailbox.uid_validity = uid_validity;
            for (i, message) in mailbox.messages.iter_mut().enumerate() {
                message.uid = u32::try_from(i).unwrap_or(u32::MAX - 1) + 1;
            }
            mailbox.uid_next = u32::try_from(mailbox.messages.len()).unwrap_or(u32::MAX - 1) + 1;
            mailbox.expunged.clear();
            mailbox.bump();
            uid_validity
        };
        self.notify();
        Some(uid_validity)
    }

    /// Deletes a mailbox with its messages (like `DELETE`). Returns whether
    /// it existed.
    pub fn delete_mailbox(&self, name: &str) -> bool {
        {
            let mut state = self.lock();
            let Some(index) = find_index(&state.mailboxes, name) else {
                return false;
            };
            state.mailboxes.remove(index);
        }
        self.notify();
        true
    }

    /// Renames a mailbox and its children (`from/...`), like `RENAME`.
    /// Renaming `INBOX` is not supported by the mock.
    pub fn rename_mailbox(&self, from: &str, to: &str) -> Result<(), MailboxError> {
        {
            let mut state = self.lock();
            if from.eq_ignore_ascii_case("INBOX") || to.eq_ignore_ascii_case("INBOX") {
                return Err(MailboxError::NotAllowed);
            }
            let index = find_index(&state.mailboxes, from).ok_or(MailboxError::NonExistent)?;
            if find_index(&state.mailboxes, to).is_some() {
                return Err(MailboxError::AlreadyExists);
            }
            let from_name = state.mailboxes[index].name.clone();
            let prefix = format!("{from_name}/");
            for mailbox in &mut state.mailboxes {
                if mailbox.name == from_name {
                    mailbox.name = to.to_owned();
                } else if let Some(rest) = mailbox.name.strip_prefix(&prefix) {
                    mailbox.name = format!("{to}/{rest}");
                }
            }
        }
        self.notify();
        Ok(())
    }

    /// Removes the message with `uid` (like `STORE \Deleted` + `EXPUNGE`).
    pub fn expunge(&self, mailbox: &str, uid: u32) -> bool {
        !self.expunge_uids(mailbox, &[uid]).is_empty()
    }

    /// Removes a message (alias of [`Self::expunge`]).
    pub fn remove_message(&self, mailbox: &str, uid: u32) -> bool {
        self.expunge(mailbox, uid)
    }

    /// Removes the messages with these UIDs; returns the UIDs that existed.
    pub fn expunge_uids(&self, mailbox: &str, uids: &[u32]) -> Vec<u32> {
        self.change(mailbox, |mailbox| {
            let wanted: BTreeSet<u32> = uids.iter().copied().collect();
            let removed: Vec<u32> = mailbox
                .messages
                .iter()
                .filter(|m| wanted.contains(&m.uid))
                .map(|m| m.uid)
                .collect();
            if !removed.is_empty() {
                mailbox.messages.retain(|m| !wanted.contains(&m.uid));
                let modseq = mailbox.bump();
                mailbox
                    .expunged
                    .extend(removed.iter().map(|&uid| (uid, modseq)));
            }
            removed
        })
        .unwrap_or_default()
    }

    /// Removes every message flagged `\Deleted` whose UID satisfies
    /// `filter`; returns the removed UIDs.
    pub(crate) fn expunge_deleted(&self, mailbox: &str, filter: impl Fn(u32) -> bool) -> Vec<u32> {
        let Some(snapshot) = self.mailbox(mailbox) else {
            return Vec::new();
        };
        let uids: Vec<u32> = snapshot
            .messages
            .iter()
            .filter(|m| m.has_flag("\\Deleted") && filter(m.uid))
            .map(|m| m.uid)
            .collect();
        if uids.is_empty() {
            return uids;
        }
        self.expunge_uids(mailbox, &uids)
    }

    /// Replaces the flags of a message (as another client would); returns
    /// whether the message exists.
    pub fn set_flags<S: AsRef<str>>(&self, mailbox: &str, uid: u32, flags: &[S]) -> bool {
        self.update_flags(mailbox, uid, FlagUpdate::Replace, flags)
    }

    /// Adds flags to a message; returns whether the message exists.
    pub fn add_flags<S: AsRef<str>>(&self, mailbox: &str, uid: u32, flags: &[S]) -> bool {
        self.update_flags(mailbox, uid, FlagUpdate::Add, flags)
    }

    /// Removes flags from a message; returns whether the message exists.
    pub fn remove_flags<S: AsRef<str>>(&self, mailbox: &str, uid: u32, flags: &[S]) -> bool {
        self.update_flags(mailbox, uid, FlagUpdate::Remove, flags)
    }

    fn update_flags<S: AsRef<str>>(
        &self,
        mailbox: &str,
        uid: u32,
        update: FlagUpdate,
        flags: &[S],
    ) -> bool {
        let flags: Vec<String> = flags.iter().map(|f| f.as_ref().to_owned()).collect();
        !self
            .store_flags(mailbox, &[uid], update, &flags, None)
            .updated
            .is_empty()
    }

    /// Changes the flags of the messages with these UIDs. With
    /// `unchanged_since` (RFC 7162), messages whose mod-sequence is higher
    /// are left alone and reported as refused. The mod-sequence only changes
    /// when the flags did.
    pub fn store_flags(
        &self,
        mailbox: &str,
        uids: &[u32],
        update: FlagUpdate,
        flags: &[String],
        unchanged_since: Option<u64>,
    ) -> StoreOutcome {
        let wanted: BTreeSet<u32> = uids.iter().copied().collect();
        let mut any_change = false;
        let result = {
            let mut state = self.lock();
            let Some(index) = find_index(&state.mailboxes, mailbox) else {
                return StoreOutcome::default();
            };
            let mailbox = &mut state.mailboxes[index];
            let mut updated = Vec::new();
            let mut refused = Vec::new();
            for i in 0..mailbox.messages.len() {
                let uid = mailbox.messages[i].uid;
                if !wanted.contains(&uid) {
                    continue;
                }
                if unchanged_since.is_some_and(|limit| mailbox.messages[i].modseq > limit) {
                    refused.push(uid);
                    continue;
                }
                let before = mailbox.messages[i].flags.clone();
                let after = apply_flags(&before, update, flags);
                if !same_flags(&before, &after) {
                    let modseq = mailbox.bump();
                    let message = &mut mailbox.messages[i];
                    message.flags = after;
                    message.modseq = modseq;
                    any_change = true;
                }
                let message = &mailbox.messages[i];
                updated.push(StoredFlags {
                    uid,
                    flags: message.flags.clone(),
                    modseq: message.modseq,
                });
            }
            StoreOutcome { updated, refused }
        };
        if any_change {
            self.notify();
        }
        result
    }

    /// Copies messages to another mailbox (flags and internal date are
    /// kept). Returns `(source UID, new UID)` pairs in source UID order.
    pub fn copy_messages(
        &self,
        from: &str,
        uids: &[u32],
        to: &str,
    ) -> Result<Vec<(u32, u32)>, MailboxError> {
        let mapping = {
            let mut state = self.lock();
            let source = find_index(&state.mailboxes, from).ok_or(MailboxError::NonExistent)?;
            let target = find_index(&state.mailboxes, to).ok_or(MailboxError::NonExistent)?;
            let wanted: BTreeSet<u32> = uids.iter().copied().collect();
            let copies: Vec<StoredMessage> = state.mailboxes[source]
                .messages
                .iter()
                .filter(|m| wanted.contains(&m.uid))
                .cloned()
                .collect();
            let target = &mut state.mailboxes[target];
            let mut mapping = Vec::with_capacity(copies.len());
            for mut message in copies {
                let old = message.uid;
                message.uid = target.uid_next;
                target.uid_next += 1;
                message.modseq = target.bump();
                mapping.push((old, message.uid));
                target.messages.push(message);
            }
            mapping
        };
        self.notify();
        Ok(mapping)
    }

    /// The flags of a message.
    pub fn flags(&self, mailbox: &str, uid: u32) -> Option<Vec<String>> {
        let state = self.lock();
        let index = find_index(&state.mailboxes, mailbox)?;
        state.mailboxes[index].message(uid).map(|m| m.flags.clone())
    }

    /// The `HIGHESTMODSEQ` of a mailbox.
    pub fn highest_modseq(&self, mailbox: &str) -> Option<u64> {
        let state = self.lock();
        find_index(&state.mailboxes, mailbox).map(|i| state.mailboxes[i].highest_modseq)
    }

    /// A snapshot of one mailbox.
    pub fn mailbox(&self, name: &str) -> Option<Mailbox> {
        let state = self.lock();
        find_index(&state.mailboxes, name).map(|i| state.mailboxes[i].clone())
    }

    /// A snapshot of all mailboxes, in creation order.
    pub fn mailboxes(&self) -> Vec<Mailbox> {
        self.lock().mailboxes.clone()
    }
}

/// Deduplicates flags case-insensitively, keeping the first spelling;
/// `\Recent` can never be stored.
fn normalize_flags<'a>(flags: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for flag in flags {
        if flag.eq_ignore_ascii_case("\\Recent") || out.iter().any(|f| f.eq_ignore_ascii_case(flag))
        {
            continue;
        }
        out.push(flag.to_owned());
    }
    out
}

fn apply_flags(current: &[String], update: FlagUpdate, flags: &[String]) -> Vec<String> {
    match update {
        FlagUpdate::Replace => normalize_flags(flags.iter().map(String::as_str)),
        FlagUpdate::Add => normalize_flags(current.iter().chain(flags).map(String::as_str)),
        FlagUpdate::Remove => current
            .iter()
            .filter(|f| !flags.iter().any(|r| r.eq_ignore_ascii_case(f)))
            .cloned()
            .collect(),
    }
}

fn same_flags(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|f| b.iter().any(|g| g.eq_ignore_ascii_case(f)))
}

fn find_index(mailboxes: &[Mailbox], name: &str) -> Option<usize> {
    mailboxes.iter().position(|m| {
        if name.eq_ignore_ascii_case("INBOX") {
            m.name == "INBOX"
        } else {
            m.name == name
        }
    })
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Derives the IMAP `INTERNALDATE` from the `Date` header.
fn internal_date_of(raw: &[u8]) -> String {
    let parsed = MessageParser::default().parse_headers(raw);
    let Some(date) = parsed
        .as_ref()
        .and_then(|m| m.date())
        .filter(|d| d.is_valid())
    else {
        return FALLBACK_INTERNAL_DATE.to_owned();
    };
    let sign = if date.tz_before_gmt && (date.tz_hour > 0 || date.tz_minute > 0) {
        '-'
    } else {
        '+'
    };
    format!(
        "{:02}-{}-{:04} {:02}:{:02}:{:02} {sign}{:02}{:02}",
        date.day,
        MONTHS[usize::from(date.month.clamp(1, 12)) - 1],
        date.year,
        date.hour,
        date.minute,
        date.second,
        date.tz_hour,
        date.tz_minute
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: &[&str] = &[];

    #[test]
    fn demo_store_assigns_sequential_uids() {
        let store = MailStore::demo();
        let inbox = store.mailbox("inbox").unwrap();
        assert_eq!(inbox.name, "INBOX");
        let uids: Vec<_> = inbox.messages.iter().map(|m| m.uid).collect();
        let expected: Vec<u32> = (1..=u32::try_from(uids.len()).unwrap()).collect();
        assert_eq!(uids, expected);
        assert_eq!(inbox.uid_next, expected.len() as u32 + 1);
        assert_eq!(store.mailboxes().len(), 3);
    }

    #[test]
    fn append_and_reset_uid_validity() {
        let store = MailStore::new();
        assert_eq!(
            store.append("INBOX", b"Subject: a\n\nx", &["\\Seen"]),
            Some(1)
        );
        assert_eq!(store.append("INBOX", b"Subject: b\n\nx", NONE), Some(2));
        assert!(store.expunge("INBOX", 1));
        let before = store.mailbox("INBOX").unwrap().uid_validity;
        let after = store.reset_uid_validity("INBOX").unwrap();
        assert_ne!(before, after);
        let inbox = store.mailbox("INBOX").unwrap();
        assert_eq!(inbox.messages[0].uid, 1);
        assert_eq!(inbox.uid_next, 2);
        assert_eq!(store.append("Nope", b"x", NONE), None);
    }

    #[test]
    fn internal_date_from_header() {
        assert_eq!(
            internal_date_of(b"Date: Tue, 22 Sep 2026 17:42:10 -0400\r\n\r\n"),
            "22-Sep-2026 17:42:10 -0400"
        );
        assert_eq!(
            internal_date_of(b"Date: garbage\r\n\r\n"),
            FALLBACK_INTERNAL_DATE
        );
        assert_eq!(internal_date_of(b""), FALLBACK_INTERNAL_DATE);
    }

    #[test]
    fn add_mailbox_is_idempotent() {
        let store = MailStore::new();
        assert!(!store.add_mailbox("inbox", FolderRole::Inbox));
        assert!(store.add_mailbox("Work/Reports", FolderRole::Other));
        assert!(!store.add_mailbox("Work/Reports", FolderRole::Other));
    }

    #[test]
    fn flag_changes_bump_modseq_only_when_flags_change() {
        let store = MailStore::new();
        let uid = store.append("INBOX", b"Subject: a\r\n\r\nx", NONE).unwrap();
        let start = store.highest_modseq("INBOX").unwrap();
        assert!(store.add_flags("INBOX", uid, &["\\Seen", "$Label1"]));
        let after_add = store.highest_modseq("INBOX").unwrap();
        assert!(after_add > start);
        assert_eq!(store.flags("INBOX", uid).unwrap(), ["\\Seen", "$Label1"]);
        // Adding what is already there (other case) changes nothing.
        assert!(store.add_flags("INBOX", uid, &["\\SEEN"]));
        assert_eq!(store.highest_modseq("INBOX").unwrap(), after_add);
        assert!(store.remove_flags("INBOX", uid, &["$label1"]));
        assert_eq!(store.flags("INBOX", uid).unwrap(), ["\\Seen"]);
        assert!(store.set_flags("INBOX", uid, &["\\Flagged", "\\Recent"]));
        assert_eq!(store.flags("INBOX", uid).unwrap(), ["\\Flagged"]);
        assert!(!store.set_flags("INBOX", 99, &["\\Seen"]));
    }

    #[test]
    fn conditional_store_refuses_newer_messages() {
        let store = MailStore::new();
        let a = store.append("INBOX", b"Subject: a\r\n\r\nx", NONE).unwrap();
        let b = store.append("INBOX", b"Subject: b\r\n\r\nx", NONE).unwrap();
        let modseq_a = store.mailbox("INBOX").unwrap().message(a).unwrap().modseq;
        let outcome = store.store_flags(
            "INBOX",
            &[a, b],
            FlagUpdate::Add,
            &["\\Seen".to_owned()],
            Some(modseq_a),
        );
        assert_eq!(outcome.refused, [b]);
        assert_eq!(outcome.updated.len(), 1);
        assert_eq!(outcome.updated[0].uid, a);
    }

    #[test]
    fn expunge_records_vanished_and_notifies() {
        let store = MailStore::new();
        let mut rx = store.subscribe();
        rx.borrow_and_update();
        let uid = store.append("INBOX", b"Subject: a\r\n\r\nx", NONE).unwrap();
        assert!(rx.has_changed().unwrap());
        rx.borrow_and_update();
        assert!(store.remove_message("INBOX", uid));
        assert!(rx.has_changed().unwrap());
        let inbox = store.mailbox("INBOX").unwrap();
        assert_eq!(inbox.expunged, [(uid, inbox.highest_modseq)]);
        assert!(store.expunge_uids("INBOX", &[uid]).is_empty());
    }

    #[test]
    fn copy_and_rename() {
        let store = MailStore::new();
        store.add_mailbox("Work", FolderRole::Other);
        store.add_mailbox("Work/Old", FolderRole::Other);
        let a = store
            .append("INBOX", b"Subject: a\r\n\r\nx", &["\\Seen"])
            .unwrap();
        assert_eq!(
            store.copy_messages("INBOX", &[a], "Work").unwrap(),
            [(a, 1)]
        );
        assert_eq!(store.flags("Work", 1).unwrap(), ["\\Seen"]);
        assert_eq!(
            store.copy_messages("INBOX", &[a], "Nope"),
            Err(MailboxError::NonExistent)
        );
        store.rename_mailbox("Work", "Jobs").unwrap();
        let names: Vec<String> = store.mailboxes().into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["INBOX", "Jobs", "Jobs/Old"]);
        assert_eq!(
            store.rename_mailbox("Jobs", "INBOX"),
            Err(MailboxError::NotAllowed)
        );
        assert_eq!(
            store.rename_mailbox("Nope", "X"),
            Err(MailboxError::NonExistent)
        );
        store.add_mailbox("X", FolderRole::Other);
        assert_eq!(
            store.rename_mailbox("Jobs", "X"),
            Err(MailboxError::AlreadyExists)
        );
    }
}
