//! The decrypted-content indexing policy (M2a WP33, plan section 7;
//! ADR 0032).
//!
//! End-to-end encrypted mail is protected on the server and on the wire;
//! putting its decrypted text into the search index would copy it to disk
//! in a readable form. So:
//!
//! * by default decrypted content is **not** indexed: an encrypted message
//!   is findable by what is outside the encryption only (addresses and the
//!   outer subject — `...` when headers are protected);
//! * an account can opt in
//!   ([`nail_api::security::SecuritySettings::index_decrypted`]); its
//!   decrypted messages are then indexed in full (protected subject, body,
//!   attachment names) and marked, and the index **counts as sensitive**:
//!   the UI says so, and with encryption at rest on it is encrypted like
//!   the rest;
//! * withdrawing the opt-in removes the marked documents of that account
//!   ([`IndexPolicy::withdrawn`] → [`crate::SearchIndex::remove_decrypted`]);
//!   the caller re-adds the messages from their outer content.

use std::collections::BTreeSet;

use nail_api::security::SecuritySettings;
use nail_api::{AccountId, MessageId};
use nail_mime::ParsedMessage;

use crate::document::IndexDocument;

/// Which accounts may have decrypted content in the index.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexPolicy {
    decrypted: BTreeSet<AccountId>,
}

impl IndexPolicy {
    /// The policy of the stored security settings.
    pub fn from_settings(settings: &SecuritySettings) -> Self {
        Self {
            decrypted: settings.index_decrypted.iter().cloned().collect(),
        }
    }

    /// Whether decrypted content of `account` may be indexed.
    pub fn indexes_decrypted(&self, account: &AccountId) -> bool {
        self.decrypted.contains(account)
    }

    /// Whether the index may hold decrypted content (it then counts as
    /// sensitive).
    pub fn index_is_sensitive(&self) -> bool {
        !self.decrypted.is_empty()
    }

    /// Accounts that opted in under `previous` and no longer do: their
    /// decrypted documents must be removed and re-added from the outer
    /// content.
    pub fn withdrawn(&self, previous: &IndexPolicy) -> Vec<AccountId> {
        previous
            .decrypted
            .difference(&self.decrypted)
            .cloned()
            .collect()
    }

    /// Accounts that opted in now: their encrypted messages should be
    /// reindexed with the decrypted content.
    pub fn granted(&self, previous: &IndexPolicy) -> Vec<AccountId> {
        self.decrypted
            .difference(&previous.decrypted)
            .cloned()
            .collect()
    }

    /// The document for a message. `decrypted` is the decrypted content of
    /// an encrypted message (`None` for mail that is not encrypted, or that
    /// could not be decrypted). Without the account's opt-in only the outer
    /// addresses and subject are indexed; the outer body (ciphertext or a
    /// placeholder) and outer attachment names (`encrypted.asc`) are not.
    pub fn document(
        &self,
        message_id: MessageId,
        account_id: AccountId,
        outer: &ParsedMessage,
        decrypted: Option<&ParsedMessage>,
    ) -> IndexDocument {
        let mut doc = IndexDocument::from_parsed(message_id, account_id.clone(), outer);
        let Some(inner) = decrypted else {
            return doc;
        };
        doc.body = String::new();
        doc.attachment_names = Vec::new();
        if !self.indexes_decrypted(&account_id) {
            return doc;
        }
        let inner_doc = IndexDocument::from_parsed(doc.message_id.clone(), account_id, inner);
        if !inner_doc.subject.is_empty() {
            // Header protection: the real subject is inside.
            doc.subject = inner_doc.subject;
        }
        doc.body = inner_doc.body;
        doc.attachment_names = inner_doc.attachment_names;
        doc.decrypted = true;
        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outer() -> ParsedMessage {
        nail_mime::parse_message(
            b"From: Ada <ada@example.org>\r\nTo: bob@example.org\r\nSubject: ...\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=b\r\n\r\n\
--b\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n\
--b\r\nContent-Type: application/octet-stream; name=\"encrypted.asc\"\r\n\
Content-Disposition: inline; filename=\"encrypted.asc\"\r\n\r\n\
-----BEGIN PGP MESSAGE-----\r\nhQEMA\r\n-----END PGP MESSAGE-----\r\n--b--\r\n",
        )
    }

    fn inner() -> ParsedMessage {
        nail_mime::parse_message(
            b"Subject: Launch plan\r\nContent-Type: multipart/mixed; boundary=c\r\n\r\n\
--c\r\nContent-Type: text/plain\r\n\r\nThe rocket leaves at dawn.\r\n\
--c\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"route.pdf\"\r\n\r\n%PDF\r\n--c--\r\n",
        )
    }

    fn settings(accounts: &[&str]) -> SecuritySettings {
        SecuritySettings {
            index_decrypted: accounts.iter().map(|a| AccountId::from(*a)).collect(),
            ..SecuritySettings::default()
        }
    }

    #[test]
    fn off_by_default_indexes_only_outer_headers() {
        let policy = IndexPolicy::from_settings(&SecuritySettings::default());
        assert!(!policy.index_is_sensitive());
        let doc = policy.document("m1".into(), "a1".into(), &outer(), Some(&inner()));
        assert!(!doc.decrypted);
        assert_eq!(doc.subject, "...");
        assert!(doc.from.contains("ada@example.org"));
        assert!(doc.body.is_empty(), "{:?}", doc.body);
        assert!(doc.attachment_names.is_empty());
    }

    #[test]
    fn opted_in_account_indexes_decrypted_content_and_marks_it() {
        let policy = IndexPolicy::from_settings(&settings(&["a1"]));
        assert!(policy.index_is_sensitive());
        let doc = policy.document("m1".into(), "a1".into(), &outer(), Some(&inner()));
        assert!(doc.decrypted);
        assert_eq!(doc.subject, "Launch plan");
        assert!(doc.body.contains("rocket leaves"));
        assert_eq!(doc.attachment_names, ["route.pdf"]);
        assert!(doc.to.contains("bob@example.org"));

        // Another account is not opted in.
        let doc = policy.document("m2".into(), "a2".into(), &outer(), Some(&inner()));
        assert!(!doc.decrypted && doc.body.is_empty());
    }

    #[test]
    fn plain_mail_is_indexed_as_before() {
        let policy = IndexPolicy::default();
        let doc = policy.document("m1".into(), "a1".into(), &inner(), None);
        assert!(!doc.decrypted);
        assert!(doc.body.contains("rocket"));
    }

    #[test]
    fn withdrawn_and_granted_accounts() {
        let before = IndexPolicy::from_settings(&settings(&["a1", "a2"]));
        let after = IndexPolicy::from_settings(&settings(&["a2", "a3"]));
        assert_eq!(after.withdrawn(&before), vec![AccountId::from("a1")]);
        assert_eq!(after.granted(&before), vec![AccountId::from("a3")]);
    }
}
