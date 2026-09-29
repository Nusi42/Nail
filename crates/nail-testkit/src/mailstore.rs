//! The in-memory mailbox state served by the mock IMAP server.
//!
//! [`MailStore`] is cheaply cloneable and shared between the server and the
//! test, so tests can append messages or reset UIDVALIDITY while clients are
//! connected.

use std::sync::{Arc, Mutex, MutexGuard};

use mail_parser::MessageParser;
use nail_api::FolderRole;

use crate::corpus::{CorpusFolder, demo_corpus, to_crlf};

/// Default internal date for messages without a usable `Date` header.
const FALLBACK_INTERNAL_DATE: &str = "01-Jan-2026 00:00:00 +0000";

/// One stored message.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    /// The message UID (unique within the mailbox's UIDVALIDITY epoch).
    pub uid: u32,
    /// IMAP flags as atoms, e.g. `\Seen`.
    pub flags: Vec<String>,
    /// IMAP `INTERNALDATE`, e.g. `28-Sep-2026 08:15:00 +0200`.
    pub internal_date: String,
    /// Raw RFC 5322 bytes (CRLF).
    pub raw: Arc<Vec<u8>>,
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

    /// Creates a mailbox if it does not exist yet (`INBOX` is matched
    /// case-insensitively). Returns `false` when it already existed.
    pub fn add_mailbox(&self, name: &str, role: FolderRole) -> bool {
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
        });
        true
    }

    /// Appends a message and returns its UID, or `None` when the mailbox does
    /// not exist. Line endings are normalized to CRLF.
    pub fn append<S: AsRef<str>>(&self, mailbox: &str, raw: &[u8], flags: &[S]) -> Option<u32> {
        let raw = to_crlf(raw);
        let internal_date = internal_date_of(&raw);
        let mut state = self.lock();
        let index = find_index(&state.mailboxes, mailbox)?;
        let mailbox = &mut state.mailboxes[index];
        let uid = mailbox.uid_next;
        mailbox.uid_next += 1;
        mailbox.messages.push(StoredMessage {
            uid,
            flags: flags.iter().map(|f| f.as_ref().to_owned()).collect(),
            internal_date,
            raw: Arc::new(raw),
        });
        Some(uid)
    }

    /// Simulates a server-side mailbox rebuild: assigns a new UIDVALIDITY and
    /// renumbers all messages from UID 1. Returns the new UIDVALIDITY.
    pub fn reset_uid_validity(&self, mailbox: &str) -> Option<u32> {
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
        Some(uid_validity)
    }

    /// Removes the message with `uid` (like `STORE \Deleted` + `EXPUNGE`).
    pub fn expunge(&self, mailbox: &str, uid: u32) -> bool {
        let mut state = self.lock();
        let Some(index) = find_index(&state.mailboxes, mailbox) else {
            return false;
        };
        let messages = &mut state.mailboxes[index].messages;
        let before = messages.len();
        messages.retain(|m| m.uid != uid);
        messages.len() != before
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

    /// Runs `f` on a mailbox under the lock (used by the server for flag updates).
    pub(crate) fn with_mailbox_mut<R>(
        &self,
        name: &str,
        f: impl FnOnce(&mut Mailbox) -> R,
    ) -> Option<R> {
        let mut state = self.lock();
        let index = find_index(&state.mailboxes, name)?;
        Some(f(&mut state.mailboxes[index]))
    }
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
        assert_eq!(
            store.append("INBOX", b"Subject: b\n\nx", &[] as &[&str]),
            Some(2)
        );
        assert!(store.expunge("INBOX", 1));
        let before = store.mailbox("INBOX").unwrap().uid_validity;
        let after = store.reset_uid_validity("INBOX").unwrap();
        assert_ne!(before, after);
        let inbox = store.mailbox("INBOX").unwrap();
        assert_eq!(inbox.messages[0].uid, 1);
        assert_eq!(inbox.uid_next, 2);
        assert_eq!(store.append("Nope", b"x", &[] as &[&str]), None);
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
}
