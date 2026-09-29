//! What the index stores about a message.

use nail_api::{AccountId, Address, MessageId};
use nail_mime::ParsedMessage;

/// The searchable text of one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDocument {
    /// The message in the store.
    pub message_id: MessageId,
    /// Its account (to drop an account's documents at once).
    pub account_id: AccountId,
    /// Sender names and addresses.
    pub from: String,
    /// `To` names and addresses.
    pub to: String,
    /// `Cc` names and addresses.
    pub cc: String,
    /// Decoded subject.
    pub subject: String,
    /// Decoded body text (the HTML part's text when there is no plain part).
    pub body: String,
    /// Attachment file names.
    pub attachment_names: Vec<String>,
    /// Built from decrypted content (an encrypted message, indexed because
    /// the account opted in; see [`crate::IndexPolicy`]).
    pub decrypted: bool,
}

fn addresses(list: &[Address]) -> String {
    list.iter()
        .map(|a| match &a.name {
            Some(name) => format!("{name} <{}>", a.email),
            None => a.email.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl IndexDocument {
    /// Builds the document from a parsed message.
    pub fn from_parsed(
        message_id: MessageId,
        account_id: AccountId,
        parsed: &ParsedMessage,
    ) -> Self {
        Self {
            message_id,
            account_id,
            from: addresses(&parsed.headers.from),
            to: addresses(&parsed.headers.to),
            cc: addresses(&parsed.headers.cc),
            subject: parsed.headers.subject.clone(),
            body: parsed.text.clone(),
            attachment_names: parsed
                .attachments
                .iter()
                .filter_map(|a| a.filename.clone())
                .collect(),
            decrypted: false,
        }
    }

    /// Parses raw RFC 5322 bytes and builds the document.
    pub fn from_raw(message_id: MessageId, account_id: AccountId, raw: &[u8]) -> Self {
        Self::from_parsed(message_id, account_id, &nail_mime::parse_message(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_from_raw_mail() {
        let raw = b"From: Ada Lovelace <ada@example.org>\r\n\
To: bob@example.org\r\n\
Cc: Carol <carol@example.org>\r\n\
Subject: =?utf-8?q?Gr=C3=BC=C3=9Fe?=\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=b\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
The engine is ready.\r\n\
--b\r\n\
Content-Type: application/pdf\r\n\
Content-Disposition: attachment; filename=\"notes.pdf\"\r\n\
\r\n\
%PDF\r\n\
--b--\r\n";
        let doc = IndexDocument::from_raw("m1".into(), "a1".into(), raw);
        assert_eq!(doc.from, "Ada Lovelace <ada@example.org>");
        assert_eq!(doc.to, "bob@example.org");
        assert_eq!(doc.cc, "Carol <carol@example.org>");
        assert_eq!(doc.subject, "Grüße");
        assert!(doc.body.contains("engine is ready"), "{:?}", doc.body);
        assert_eq!(doc.attachment_names, ["notes.pdf"]);
    }
}
