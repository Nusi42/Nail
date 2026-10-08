//! The connection log, error details and the application log (ADR 0190).
//!
//! Every protocol client records the commands it sends and the response
//! lines it receives, **redacted** by `nail-connlog` (no credentials, no mail
//! content, masked addresses), into a bounded in-memory ring per
//! [`ConnectionLogScope`]. The rings are never written to disk unless the user
//! turns on [`ConnectionLogSettings::keep_on_disk`].
//!
//! Failures carry [`ErrorDetails`] (step, folder, command, the server's text);
//! the last ones per account and folder are stored ([`SyncErrorRecord`]) so
//! they survive restarts. Remote UI sessions see the log only with the
//! `admin` grant; MCP and plug-ins never.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ErrorCode;
use crate::ids::{AccountId, FolderId, PimSourceId};

/// Entries kept in memory per scope.
pub const CONNECTION_LOG_CAPACITY: usize = 2000;

/// Longest logged text of one entry (bytes); longer ones are cut and end
/// with the number of bytes left out.
pub const CONNECTION_LOG_LINE_BYTES: usize = 1000;

/// Size at which a connection log file on disk is rotated.
pub const CONNECTION_LOG_FILE_BYTES: u64 = 1024 * 1024;

/// Size at which `app.log` is rotated.
pub const APP_LOG_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Older files kept next to a rotated log (`.1`, `.2`).
pub const LOG_FILE_GENERATIONS: usize = 2;

/// Name of the log folder in the data directory.
pub const LOG_DIR: &str = "logs";

/// Name of the application log file in [`LOG_DIR`].
pub const APP_LOG_FILE: &str = "app.log";

/// Name of the connection log folder in [`LOG_DIR`].
pub const CONNECTION_LOG_DIR: &str = "connections";

/// Recent `app.log` lines [`crate::Request::GetAppLog`] returns at most.
pub const APP_LOG_LINES: usize = 500;

/// The placeholder of a removed credential (also used by the diagnostics
/// bundle).
pub const REDACTED: &str = "<redacted>";

labeled_enum! {
    /// The protocol of a log entry (`connlog-protocol-*`).
    pub enum WireProtocol : "connlog-protocol" {
        /// IMAP.
        Imap = "imap",
        /// SMTP submission.
        Smtp = "smtp",
        /// POP3.
        Pop3 = "pop3",
        /// ManageSieve.
        ManageSieve = "managesieve",
        /// CalDAV.
        CalDav = "caldav",
        /// CardDAV.
        CardDav = "carddav",
        /// Calendar subscriptions (ICS over HTTP).
        Feed = "feed",
        /// The Gmail API.
        Gmail = "gmail",
        /// Microsoft Graph.
        Graph = "graph",
        /// Exchange Web Services.
        Ews = "ews",
        /// OAuth token endpoints.
        OAuth = "oauth",
        /// Autodiscover, autoconfig, ISPDB and DNS lookups.
        Discovery = "discovery",
    }
}

labeled_enum! {
    /// Who wrote a log entry (`connlog-direction-*`).
    pub enum WireDirection : "connlog-direction" {
        /// A command or request Nail sent.
        Sent = "sent",
        /// A response line or answer the server sent.
        Received = "received",
        /// A note of Nail's own (connected, TLS, closed, an error).
        Info = "info",
    }
}

labeled_enum! {
    /// How notable a log entry is (`connlog-level-*`).
    pub enum WireLevel : "connlog-level" {
        /// Normal traffic.
        Info = "info",
        /// A refusal or something unexpected that Nail worked around.
        Warning = "warning",
        /// A failure.
        Error = "error",
    }
}

/// Whose connection log: an account, a calendar / address book source of
/// its own, or account setup (discovery and test logins before an account
/// exists).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ConnectionLogScope {
    /// A mail account (also its CalDAV / CardDAV sources and OAuth tokens).
    Account {
        /// The account.
        account_id: AccountId,
    },
    /// A PIM source without an account of its own.
    PimSource {
        /// The source.
        source_id: PimSourceId,
    },
    /// Account setup: discovery and test logins.
    Setup,
}

impl ConnectionLogScope {
    /// The scope of an account.
    pub fn account(account_id: &AccountId) -> Self {
        Self::Account {
            account_id: account_id.clone(),
        }
    }

    /// The account this scope belongs to, if any.
    pub fn account_id(&self) -> Option<&AccountId> {
        match self {
            Self::Account { account_id } => Some(account_id),
            _ => None,
        }
    }
}

/// One redacted line of the connection log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConnectionLogEntry {
    /// Position in the scope's log (increasing, never reused while the
    /// process runs).
    pub seq: u64,
    /// When it was recorded (RFC 3339, milliseconds, UTC).
    pub at: String,
    /// The protocol.
    pub protocol: WireProtocol,
    /// The connection (or HTTP exchange) it belongs to, unique while the
    /// process runs.
    pub connection: u32,
    /// Sent, received or a note.
    pub direction: WireDirection,
    /// Normal, warning or error.
    pub level: WireLevel,
    /// The redacted text.
    pub text: String,
}

impl ConnectionLogEntry {
    /// The entry as one text line (copy, export and the files on disk):
    /// `2026-10-08T09:12:01.123Z imap #3 C: A0004 SELECT "INBOX"`.
    pub fn line(&self) -> String {
        let marker = match self.direction {
            WireDirection::Sent => "C:",
            WireDirection::Received => "S:",
            WireDirection::Info => match self.level {
                WireLevel::Info => "--",
                WireLevel::Warning => "!!",
                WireLevel::Error => "XX",
            },
        };
        format!(
            "{} {} #{} {marker} {}",
            self.at,
            self.protocol.as_str(),
            self.connection,
            self.text
        )
    }
}

/// Answer to [`crate::Request::GetConnectionLog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConnectionLog {
    /// Whose log.
    pub scope: ConnectionLogScope,
    /// Entries after the requested position, oldest first.
    pub entries: Vec<ConnectionLogEntry>,
    /// The newest position (0 = nothing recorded yet).
    pub last_seq: u64,
    /// Entries already dropped from memory (the ring keeps `capacity`).
    pub dropped: u64,
    /// Entries kept in memory per scope ([`CONNECTION_LOG_CAPACITY`]).
    pub capacity: u32,
    /// Whether the log is also kept on disk.
    pub keep_on_disk: bool,
}

/// The connection log settings (app setting; off = memory only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConnectionLogSettings {
    /// "Keep connection log on disk": append the redacted lines to rotating
    /// files in `<data-dir>/logs/connections/`. Off by default; turning it
    /// off deletes the files.
    #[serde(default)]
    pub keep_on_disk: bool,
}

labeled_enum! {
    /// What Nail was doing when an error happened (`error-step-*`).
    pub enum ErrorStep : "error-step" {
        /// Reaching the server.
        Connect = "connect",
        /// The TLS handshake / certificate check.
        Tls = "tls",
        /// Signing in.
        Login = "login",
        /// Getting a sign-in token.
        Token = "token",
        /// Listing folders.
        List = "list",
        /// Opening a folder.
        Select = "select",
        /// Looking for new messages.
        Search = "search",
        /// Downloading messages.
        Fetch = "fetch",
        /// Checking flags and removed messages.
        Flags = "flags",
        /// Saving downloaded messages locally.
        Store = "store",
        /// Writing a change to the server.
        Write = "write",
        /// Sending a message.
        Send = "send",
        /// Waiting for new mail.
        Idle = "idle",
        /// Finding the account's servers.
        Discover = "discover",
        /// Anything else.
        Other = "other",
    }
}

/// The precise cause of a failure, for "Show details" (ADR 0190).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ErrorDetails {
    /// The error class.
    pub code: ErrorCode,
    /// The protocol, when a server was involved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub protocol: Option<WireProtocol>,
    /// What Nail was doing.
    pub step: ErrorStep,
    /// The folder (server path), when one was involved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder: Option<String>,
    /// The command that failed, redacted
    /// (`A0012 UID FETCH 151:200 (UID FLAGS …)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command: Option<String>,
    /// The server's answer, verbatim but redacted
    /// (`NO [SERVERBUG] Internal error occurred.`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub server_text: Option<String>,
    /// Nail's own diagnostic text (English, redacted).
    pub detail: String,
    /// When it happened (RFC 3339).
    pub at: String,
    /// The connection log's position at the failure (its scope is the
    /// account's), for "Show in connection log".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub log_seq: Option<u64>,
}

/// The last error of an account (`folder_id` = `null`) or of one of its
/// folders, stored until a later sync of it succeeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SyncErrorRecord {
    /// The account.
    pub account_id: AccountId,
    /// The folder, `null` for the account as a whole.
    pub folder_id: Option<FolderId>,
    /// What went wrong.
    pub details: ErrorDetails,
}

/// A message a folder sync skipped because it could not be downloaded or
/// read (ADR 0191); retried by later syncs until it works or is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SkippedMessage {
    /// The account.
    pub account_id: AccountId,
    /// The folder.
    pub folder_id: FolderId,
    /// The message's IMAP UID.
    pub uid: u32,
    /// How many syncs tried it.
    pub attempts: u32,
    /// The last failure.
    pub details: ErrorDetails,
}

/// Answer to [`crate::Request::GetAppLog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AppLog {
    /// The file (`<data-dir>/logs/app.log`), `null` when this process writes
    /// none.
    pub path: Option<String>,
    /// The most recent lines (redacted), oldest first.
    pub lines: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_trip() {
        crate::labels::assert_labels(
            WireProtocol::ALL,
            WireProtocol::as_str,
            WireProtocol::message_id,
        );
        crate::labels::assert_labels(
            WireDirection::ALL,
            WireDirection::as_str,
            WireDirection::message_id,
        );
        crate::labels::assert_labels(WireLevel::ALL, WireLevel::as_str, WireLevel::message_id);
        crate::labels::assert_labels(ErrorStep::ALL, ErrorStep::as_str, ErrorStep::message_id);
        assert_eq!(
            WireProtocol::ManageSieve.message_id(),
            "connlog-protocol-managesieve"
        );
    }

    #[test]
    fn scopes_serialize_with_a_kind() {
        let scope = ConnectionLogScope::account(&"work".into());
        assert_eq!(
            serde_json::to_value(&scope).unwrap(),
            serde_json::json!({"kind": "account", "account_id": "work"})
        );
        assert_eq!(
            serde_json::to_value(ConnectionLogScope::Setup).unwrap(),
            serde_json::json!({"kind": "setup"})
        );
        assert_eq!(scope.account_id().map(AccountId::as_str), Some("work"));
    }

    #[test]
    fn entries_format_as_one_line() {
        let mut entry = ConnectionLogEntry {
            seq: 4,
            at: "2026-10-08T09:12:01.123Z".into(),
            protocol: WireProtocol::Imap,
            connection: 3,
            direction: WireDirection::Sent,
            level: WireLevel::Info,
            text: "A0004 SELECT \"INBOX\"".into(),
        };
        assert_eq!(
            entry.line(),
            "2026-10-08T09:12:01.123Z imap #3 C: A0004 SELECT \"INBOX\""
        );
        entry.direction = WireDirection::Info;
        entry.level = WireLevel::Error;
        assert!(entry.line().contains("#3 XX "));
        assert_eq!(
            ConnectionLogSettings::default(),
            ConnectionLogSettings {
                keep_on_disk: false
            }
        );
    }
}
