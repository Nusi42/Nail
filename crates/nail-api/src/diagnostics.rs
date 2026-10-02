//! The diagnostics bundle (M4c, ADR 0094).
//!
//! A zip file the user makes on purpose ([`crate::Request::CreateDiagnosticsBundle`])
//! and may send to support. Before writing, the UI shows **exactly** what
//! goes in ([`crate::Request::PreviewDiagnostics`] → [`DiagnosticsManifest`]:
//! every file with its size, the redactions applied and a text preview);
//! the bundle then holds those files plus `manifest.json` and nothing else.
//!
//! No mail content by default: message headers / raw messages go in only
//! when the user picks [`DiagnosticsItem::MailHeaders`] /
//! [`DiagnosticsItem::MailContent`] **and** names the messages. Secrets
//! (passwords, tokens, keys, `keychain:` names) are never included; mail
//! addresses, subjects and the user's name in paths are redacted unless the
//! user turns address redaction off.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::MessageId;

/// Most messages a bundle may include.
pub const MAX_DIAGNOSTICS_MESSAGES: usize = 20;

/// Bytes of text preview per entry in the manifest.
pub const DIAGNOSTICS_PREVIEW_BYTES: usize = 4096;

/// Name of the manifest entry inside the bundle.
pub const DIAGNOSTICS_MANIFEST_ENTRY: &str = "manifest.json";

labeled_enum! {
    /// What a bundle can hold (`diagnostics-item-*`).
    pub enum DiagnosticsItem : "diagnostics-item" {
        /// Nail and Windows versions, installer kind, WebView2, CPU / memory.
        SystemInfo = "system-info",
        /// Recent log files (redacted).
        Logs = "logs",
        /// The configuration files (redacted: addresses, hosts stay).
        Config = "config",
        /// Per account: protocol, engine state, last errors (no addresses).
        AccountStatus = "account-status",
        /// Store statistics: schema version, counts, sizes, vault state.
        StoreStats = "store-stats",
        /// Pending operations: kinds, states, errors (no content).
        OperationQueue = "operation-queue",
        /// Installed plug-ins: ids, versions, states, recent errors.
        Plugins = "plugins",
        /// Headers of the named messages (opt-in).
        MailHeaders = "mail-headers",
        /// The full raw named messages (opt-in).
        MailContent = "mail-content",
    }
}

impl DiagnosticsItem {
    /// Whether it carries mail content (never on by default).
    pub fn is_mail(self) -> bool {
        matches!(self, Self::MailHeaders | Self::MailContent)
    }
}

labeled_enum! {
    /// A redaction applied to an entry (`diagnostics-redaction-*`).
    pub enum Redaction : "diagnostics-redaction" {
        /// Passwords, tokens and keychain names removed (always).
        Secrets = "secrets",
        /// Mail addresses replaced by stable placeholders.
        Addresses = "addresses",
        /// Subjects and display names removed.
        Subjects = "subjects",
        /// The user name in file paths replaced.
        Paths = "paths",
    }
}

/// What to include.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DiagnosticsOptions {
    /// The items.
    pub items: Vec<DiagnosticsItem>,
    /// The messages for the mail items (at most
    /// [`MAX_DIAGNOSTICS_MESSAGES`]).
    pub message_ids: Vec<MessageId>,
    /// Replace mail addresses (and subjects) outside the mail items.
    pub redact_addresses: bool,
}

impl Default for DiagnosticsOptions {
    fn default() -> Self {
        Self {
            items: DiagnosticsItem::ALL
                .iter()
                .copied()
                .filter(|i| !i.is_mail())
                .collect(),
            message_ids: Vec::new(),
            redact_addresses: true,
        }
    }
}

labeled_enum! {
    /// Why diagnostics options are refused (`diagnostics-issue-*`).
    pub enum DiagnosticsIssue : "diagnostics-issue" {
        /// Nothing chosen.
        NothingChosen = "nothing-chosen",
        /// A mail item without messages.
        NoMessages = "no-messages",
        /// Messages named without a mail item.
        MessagesWithoutMailItem = "messages-without-mail-item",
        /// More than [`MAX_DIAGNOSTICS_MESSAGES`] messages.
        TooManyMessages = "too-many-messages",
    }
}

impl DiagnosticsOptions {
    /// Whether any mail content goes in.
    pub fn includes_mail(&self) -> bool {
        self.items.iter().any(|i| i.is_mail())
    }

    /// Problems (empty = valid).
    pub fn issues(&self) -> Vec<DiagnosticsIssue> {
        let mut issues = Vec::new();
        if self.items.is_empty() {
            issues.push(DiagnosticsIssue::NothingChosen);
        }
        match (self.includes_mail(), self.message_ids.is_empty()) {
            (true, true) => issues.push(DiagnosticsIssue::NoMessages),
            (false, false) => issues.push(DiagnosticsIssue::MessagesWithoutMailItem),
            _ => {}
        }
        if self.message_ids.len() > MAX_DIAGNOSTICS_MESSAGES {
            issues.push(DiagnosticsIssue::TooManyMessages);
        }
        issues
    }
}

/// One file of the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiagnosticsEntry {
    /// Its path inside the zip (`logs/nail.log`, `config/accounts.kdl`).
    pub name: String,
    /// The item it belongs to.
    pub item: DiagnosticsItem,
    /// Size in bytes (after redaction).
    pub size_bytes: u64,
    /// Redactions applied.
    #[serde(default)]
    pub redactions: Vec<Redaction>,
    /// The first [`DIAGNOSTICS_PREVIEW_BYTES`] of text entries (after
    /// redaction), for the preview.
    #[serde(default)]
    pub preview: Option<String>,
}

/// Exactly what a bundle holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiagnosticsManifest {
    /// When it was assembled (RFC 3339).
    pub created_at: String,
    /// Nail's version.
    pub app_version: String,
    /// The options used.
    pub options: DiagnosticsOptions,
    /// Every file (besides `manifest.json`).
    pub entries: Vec<DiagnosticsEntry>,
}

impl DiagnosticsManifest {
    /// Total bytes of the entries.
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.size_bytes).sum()
    }
}

/// A written bundle ([`crate::Request::CreateDiagnosticsBundle`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiagnosticsBundle {
    /// Where it was written.
    pub path: String,
    /// What it holds.
    pub manifest: DiagnosticsManifest,
    /// Size of the zip.
    pub size_bytes: u64,
    /// SHA-256 of the zip (lowercase hex), to quote to support.
    pub sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use pretty_assertions::assert_eq;

    #[test]
    fn labels_match_the_wire() {
        assert_labels(
            DiagnosticsItem::ALL,
            DiagnosticsItem::as_str,
            DiagnosticsItem::message_id,
        );
        assert_labels(Redaction::ALL, Redaction::as_str, Redaction::message_id);
        assert_labels(
            DiagnosticsIssue::ALL,
            DiagnosticsIssue::as_str,
            DiagnosticsIssue::message_id,
        );
    }

    #[test]
    fn no_mail_by_default() {
        let options = DiagnosticsOptions::default();
        assert!(!options.includes_mail());
        assert!(options.redact_addresses);
        assert!(options.issues().is_empty());
        assert_eq!(options.items.len(), DiagnosticsItem::ALL.len() - 2);
        let parsed: DiagnosticsOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, options);
    }

    #[test]
    fn mail_needs_messages() {
        let mut options = DiagnosticsOptions {
            items: vec![DiagnosticsItem::MailHeaders],
            ..DiagnosticsOptions::default()
        };
        assert_eq!(options.issues(), vec![DiagnosticsIssue::NoMessages]);
        options.message_ids = (0..=MAX_DIAGNOSTICS_MESSAGES)
            .map(|n| MessageId::new(format!("m{n}")))
            .collect();
        assert_eq!(options.issues(), vec![DiagnosticsIssue::TooManyMessages]);
        options.items = vec![DiagnosticsItem::Logs];
        options.message_ids.truncate(1);
        assert_eq!(
            options.issues(),
            vec![DiagnosticsIssue::MessagesWithoutMailItem]
        );
        options.items.clear();
        options.message_ids.clear();
        assert_eq!(options.issues(), vec![DiagnosticsIssue::NothingChosen]);
    }
}
