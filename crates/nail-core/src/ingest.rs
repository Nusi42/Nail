//! Turning raw messages into stored ones: blob store + MIME parsing +
//! metadata rows. Shared by the sync engine and the demo seed.

use nail_api::{Flags, Folder};
use nail_mime::{ParsedMessage, SummaryIds};
use nail_store::{BlobStore, Store, StoredMessage};

use crate::ids;

/// A raw message as delivered by a server (or the demo corpus).
#[derive(Debug, Clone)]
pub(crate) struct IncomingMessage {
    /// UID within the folder.
    pub uid: u32,
    /// System flags.
    pub flags: Flags,
    /// Server receive date (RFC 3339), used when the `Date` header is missing.
    pub internal_date: Option<String>,
    /// Raw RFC 5322 bytes.
    pub raw: Vec<u8>,
}

/// Stores the raw bytes of `messages` in the blob store and upserts their
/// metadata into `folder` in one transaction. Blocking; run it on the
/// blocking pool. Returns the number of messages written.
pub(crate) fn ingest(
    store: &Store,
    blobs: &BlobStore,
    folder: &Folder,
    uid_validity: u32,
    messages: &[IncomingMessage],
) -> nail_store::Result<usize> {
    if messages.is_empty() {
        return Ok(0);
    }
    let mut rows = Vec::with_capacity(messages.len());
    for message in messages {
        let blob_hash = blobs.put(&message.raw)?;
        let parsed = nail_mime::parse_message(&message.raw);
        let id = ids::message_id(&folder.account_id, &folder.path, uid_validity, message.uid);
        let summary = parsed.to_summary(
            SummaryIds {
                id,
                folder_id: folder.id.clone(),
                account_id: folder.account_id.clone(),
            },
            message.flags,
            message.internal_date.as_deref(),
        );
        rows.push(StoredMessage {
            summary,
            uid: Some(message.uid),
            blob_hash: Some(blob_hash),
            thread_key: thread_key(&parsed),
        });
    }
    store.upsert_messages(&rows)?;
    Ok(rows.len())
}

/// The thread grouping key: the root of the `References` chain, else the
/// parent from `In-Reply-To`, else the message's own `Message-ID`. Full JWZ
/// threading (with subject fallback) arrives in M1; this key already groups
/// well-formed threads.
fn thread_key(parsed: &ParsedMessage) -> Option<String> {
    parsed
        .references
        .first()
        .or_else(|| parsed.in_reply_to.first())
        .cloned()
        .or_else(|| parsed.headers.message_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_key_prefers_the_reference_root() {
        let reply = nail_mime::parse_message(
            b"Message-ID: <c@x>\r\nIn-Reply-To: <b@x>\r\nReferences: <a@x> <b@x>\r\n\r\nhi",
        );
        assert_eq!(thread_key(&reply).as_deref(), Some("a@x"));

        let answer = nail_mime::parse_message(b"Message-ID: <c@x>\r\nIn-Reply-To: <b@x>\r\n\r\nhi");
        assert_eq!(thread_key(&answer).as_deref(), Some("b@x"));

        let root = nail_mime::parse_message(b"Message-ID: <a@x>\r\n\r\nhi");
        assert_eq!(thread_key(&root).as_deref(), Some("a@x"));

        assert_eq!(thread_key(&nail_mime::parse_message(b"\r\nhi")), None);
    }
}
