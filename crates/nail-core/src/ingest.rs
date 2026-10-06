//! Turning raw messages into stored ones: blob store + MIME parsing +
//! metadata rows (with threading headers) + the search index. Shared by the
//! IMAP and POP3 engines, the outbox (local Sent copies) and the demo seed.
//!
//! Two steps (post-M4c performance work, ADR 0101):
//!
//! 1. [`prepare`] writes the blobs and parses the MIME of a batch on
//!    several threads; it needs no store, so the IMAP sync runs it while
//!    the previous batch is being stored and the next one fetched;
//! 2. [`ingest_prepared`] merges the rows (one transaction), records
//!    addresses and indexes the batch — the part that needs the store.
//!
//! [`ingest`] does both for callers with a single message or batch.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use nail_api::{Address, Flags, Folder, FolderRole, MessageId};
use nail_mime::{ParsedMessage, SummaryIds};
use nail_search::{IndexDocument, SearchIndex};
use nail_store::exchange::RemoteExtras;
use nail_store::{
    BlobHash, BlobStore, MessageFilter, MessageMeta, RemovalMark, Store, StoredMessage,
    SyncedMessage,
};

use crate::ids;

/// A raw message as delivered by a server (or the demo corpus).
#[derive(Debug, Clone, Default)]
pub(crate) struct IncomingMessage {
    /// IMAP UID within the folder; `None` for POP3 and local messages.
    pub uid: Option<u32>,
    /// The id to store it under; `None` derives it from the UID.
    pub id: Option<MessageId>,
    /// System flags.
    pub flags: Flags,
    /// Keywords (IMAP) → tags.
    pub keywords: Vec<String>,
    /// Server receive date (RFC 3339), used when the `Date` header is missing.
    pub internal_date: Option<String>,
    /// Raw RFC 5322 bytes (only the header block when `headers_only`).
    pub raw: Vec<u8>,
    /// Only the header block was fetched (sync policy `headers-only` /
    /// `recent`; the body is fetched on demand).
    pub headers_only: bool,
    /// The full message's size (`RFC822.SIZE`), when known.
    pub size: Option<u64>,
    /// The server's thread / conversation id (Gmail `threadId`, Graph /
    /// EWS conversation id; M4c): messages with the same one are one
    /// thread ([`MessageMeta::remote_thread_id`]).
    pub remote_thread_id: Option<String>,
}

/// A message whose blob is written and whose MIME is parsed
/// ([`prepare`]).
#[derive(Debug, Clone)]
pub(crate) struct PreparedMessage {
    /// The message as delivered.
    pub message: IncomingMessage,
    /// Its blob (already in the blob store).
    pub blob_hash: BlobHash,
    /// Its parsed form.
    pub parsed: ParsedMessage,
}

/// A batch [`prepare`]d for [`ingest_prepared`].
#[derive(Debug, Clone)]
pub(crate) struct PreparedBatch {
    /// The messages, in the order given.
    pub messages: Vec<PreparedMessage>,
    /// The blob store's removals before the blobs were written: only when
    /// one ran since are the blobs checked again before the rows commit.
    pub mark: RemovalMark,
}

/// Most threads [`prepare`] uses for one batch (blob writes end with an
/// fsync each, which overlap well; parsing is CPU work).
const MAX_PREPARE_THREADS: usize = 8;

/// Messages per [`prepare`] thread at least (smaller batches use fewer
/// threads; one message runs on the caller's thread).
const MIN_MESSAGES_PER_THREAD: usize = 4;

fn prepare_threads(messages: usize) -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    cpus.min(MAX_PREPARE_THREADS)
        .min(messages.div_ceil(MIN_MESSAGES_PER_THREAD))
        .max(1)
}

fn prepare_one(blobs: &BlobStore, message: IncomingMessage) -> nail_store::Result<PreparedMessage> {
    let blob_hash = blobs.put(&message.raw)?;
    let parsed = nail_mime::parse_message(&message.raw);
    Ok(PreparedMessage {
        message,
        blob_hash,
        parsed,
    })
}

/// Writes the blobs of `messages` and parses them, on up to
/// [`MAX_PREPARE_THREADS`] threads. The result keeps the order. Blocking;
/// needs no store.
pub(crate) fn prepare(
    blobs: &BlobStore,
    messages: Vec<IncomingMessage>,
) -> nail_store::Result<PreparedBatch> {
    let mark = blobs.removal_mark();
    let threads = prepare_threads(messages.len());
    if threads <= 1 {
        let messages = messages
            .into_iter()
            .map(|m| prepare_one(blobs, m))
            .collect::<nail_store::Result<_>>()?;
        return Ok(PreparedBatch { messages, mark });
    }
    let per_thread = messages.len().div_ceil(threads);
    let mut chunks: Vec<Vec<IncomingMessage>> = Vec::with_capacity(threads);
    let mut rest = messages.into_iter().peekable();
    while rest.peek().is_some() {
        chunks.push(rest.by_ref().take(per_thread).collect());
    }
    let prepared = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .into_iter()
                        .map(|m| prepare_one(blobs, m))
                        .collect::<nail_store::Result<Vec<_>>>()
                })
            })
            .collect();
        let mut out = Vec::new();
        for handle in handles {
            match handle.join() {
                Ok(part) => out.extend(part?),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
        Ok::<_, nail_store::Error>(out)
    })?;
    Ok(PreparedBatch {
        messages: prepared,
        mark,
    })
}

/// What [`ingest`] wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IngestReport {
    /// Messages written (new or updated).
    pub stored: Vec<MessageId>,
    /// The subset that did not exist before.
    pub new: Vec<MessageId>,
}

/// The threading headers and size of a parsed message.
pub(crate) fn meta_of(parsed: &ParsedMessage, size: usize) -> MessageMeta {
    MessageMeta {
        message_id: parsed.headers.message_id.clone(),
        in_reply_to: parsed.in_reply_to.clone(),
        references: parsed.references.clone(),
        size: Some(size as u64),
        remote_thread_id: None,
    }
}

/// Picks the id for an incoming message: an explicit one, the row already
/// holding this UID, a row moved here by the user whose UID is not known
/// yet (same Message-ID, no UID; the oldest), or the id derived from the
/// UID. Id-only lookups with cached statements (the sync calls this for
/// every message).
fn resolve_id(
    store: &Store,
    folder: &Folder,
    uid_validity: u32,
    message: &IncomingMessage,
    parsed: &ParsedMessage,
) -> nail_store::Result<MessageId> {
    if let Some(id) = &message.id {
        return Ok(id.clone());
    }
    let uid = message.uid.unwrap_or(0);
    if let Some(existing) = store.message_id_by_uid(&folder.id, uid)? {
        return Ok(existing);
    }
    if let Some(mid) = &parsed.headers.message_id
        && let Some(moved) = store.uidless_message_by_message_id(&folder.id, mid)?
    {
        store.set_message_uid(&moved, Some(uid))?;
        return Ok(moved);
    }
    Ok(ids::message_id(
        &folder.account_id,
        &folder.path,
        uid_validity,
        uid,
    ))
}

/// Stores the raw bytes of `messages` in the blob store and merges their
/// metadata into `folder` in one transaction (keeping pending local
/// changes, see [`Store::merge_messages`]), then indexes them. Blocking;
/// run it on the blocking pool. [`prepare`] + [`ingest_prepared`].
pub(crate) fn ingest(
    store: &Store,
    blobs: &BlobStore,
    index: Option<&SearchIndex>,
    folder: &Folder,
    uid_validity: u32,
    messages: &[IncomingMessage],
) -> nail_store::Result<IngestReport> {
    if messages.is_empty() {
        return Ok(IngestReport::default());
    }
    let batch = prepare(blobs, messages.to_vec())?;
    ingest_prepared(store, blobs, index, folder, uid_validity, &batch)
}

/// [`ingest`] with the API engine's extras per message (server id,
/// Exchange data) written in the same transaction as the messages
/// ([`Store::merge_with_remote`]). Blocking.
pub(crate) fn ingest_with_remote(
    store: &Store,
    blobs: &BlobStore,
    index: Option<&SearchIndex>,
    folder: &Folder,
    messages: &[IncomingMessage],
    extras: &BTreeMap<MessageId, RemoteExtras>,
) -> nail_store::Result<IngestReport> {
    if messages.is_empty() {
        return Ok(IngestReport::default());
    }
    let batch = prepare(blobs, messages.to_vec())?;
    merge_prepared(store, blobs, index, folder, 0, &batch, extras)
}

/// Merges a [`prepare`]d batch into `folder` (see [`ingest`]). Blocking.
pub(crate) fn ingest_prepared(
    store: &Store,
    blobs: &BlobStore,
    index: Option<&SearchIndex>,
    folder: &Folder,
    uid_validity: u32,
    batch: &PreparedBatch,
) -> nail_store::Result<IngestReport> {
    merge_prepared(
        store,
        blobs,
        index,
        folder,
        uid_validity,
        batch,
        &BTreeMap::new(),
    )
}

fn merge_prepared(
    store: &Store,
    blobs: &BlobStore,
    index: Option<&SearchIndex>,
    folder: &Folder,
    uid_validity: u32,
    batch: &PreparedBatch,
    extras: &BTreeMap<MessageId, RemoteExtras>,
) -> nail_store::Result<IngestReport> {
    let mut report = IngestReport::default();
    if batch.messages.is_empty() {
        return Ok(report);
    }
    let mut rows = Vec::with_capacity(batch.messages.len());
    let mut addresses: Vec<(MessageId, Vec<Address>)> = Vec::new();
    let mut docs = Vec::with_capacity(batch.messages.len());
    let mut keeps_body: Vec<MessageId> = Vec::new();
    for prepared in &batch.messages {
        let message = &prepared.message;
        let parsed = &prepared.parsed;
        let blob_hash = prepared.blob_hash.clone();
        let id = resolve_id(store, folder, uid_validity, message, parsed)?;
        if !store.message_exists(&id)? {
            report.new.push(id.clone());
        } else if message.headers_only
            && store
                .body_state(&id)
                .is_ok_and(|s| s != nail_api::sync::BodyState::HeadersOnly)
        {
            // A resync with headers only keeps a downloaded body (and its
            // index entry).
            keeps_body.push(id.clone());
        }
        let mut summary = parsed.to_summary(
            SummaryIds {
                id: id.clone(),
                folder_id: folder.id.clone(),
                account_id: folder.account_id.clone(),
            },
            message.flags,
            message.internal_date.as_deref(),
        );
        summary.tags.clone_from(&message.keywords);
        // Address autocomplete learns who writes to the user, and whom the
        // user writes to (Sent), never from junk.
        let learned: Vec<Address> = match folder.role {
            FolderRole::Sent => parsed
                .headers
                .to
                .iter()
                .chain(&parsed.headers.cc)
                .cloned()
                .collect(),
            FolderRole::Junk | FolderRole::Trash | FolderRole::Drafts => Vec::new(),
            _ => parsed.headers.from.clone(),
        };
        addresses.push((id.clone(), learned));
        docs.push(IndexDocument::from_parsed(
            id.clone(),
            folder.account_id.clone(),
            parsed,
        ));
        rows.push(SyncedMessage {
            message: StoredMessage {
                summary,
                uid: message.uid,
                blob_hash: Some(blob_hash),
                thread_key: thread_key(parsed),
            },
            meta: Some(MessageMeta {
                remote_thread_id: message.remote_thread_id.clone(),
                ..meta_of(
                    parsed,
                    message
                        .size
                        .filter(|_| message.headers_only)
                        .map_or(message.raw.len(), |s| {
                            usize::try_from(s).unwrap_or(usize::MAX)
                        }),
                )
            }),
        });
    }
    let mut merged = nail_store::MergeReport::default();
    for (headers_only, state) in [
        (false, nail_api::sync::BodyState::Synced),
        (true, nail_api::sync::BodyState::HeadersOnly),
    ] {
        let group: Vec<SyncedMessage> = rows
            .iter()
            .zip(&batch.messages)
            .filter(|(_, m)| m.message.headers_only == headers_only)
            .map(|(r, _)| r.clone())
            .collect();
        if group.is_empty() {
            continue;
        }
        let part = store.merge_with_remote(&group, state, extras)?;
        merged.written += part.written;
        merged.kept_local.extend(part.kept_local);
        merged.skipped.extend(part.skipped);
    }
    // Blob garbage collection (after folder deletions) may have removed a
    // blob between `put` and the row that references it; store it again.
    // Only a removal since the batch was prepared can have done that.
    if blobs.removed_since(batch.mark) {
        for (prepared, row) in batch.messages.iter().zip(&rows) {
            if let Some(hash) = &row.message.blob_hash
                && !blobs.contains(hash)
            {
                blobs.put(&prepared.message.raw)?;
            }
        }
    }
    // WP123: sets, not `Vec::contains` in loops (large batches).
    let skipped: std::collections::HashSet<&MessageId> = merged.skipped.iter().collect();
    report.new.retain(|id| !skipped.contains(id));
    for row in &rows {
        if !skipped.contains(&row.message.summary.id) {
            report.stored.push(row.message.summary.id.clone());
        }
    }
    let new: std::collections::HashSet<&MessageId> = report.new.iter().collect();
    let learned: Vec<Address> = addresses
        .into_iter()
        .filter(|(id, _)| new.contains(id))
        .flat_map(|(_, a)| a)
        .collect();
    if !learned.is_empty()
        && let Err(err) = store.record_addresses(&learned, crate::engine::unix_now())
    {
        tracing::debug!(%err, "cannot record addresses");
    }
    if let Some(index) = index {
        let stored: std::collections::HashSet<&MessageId> = report.stored.iter().collect();
        let keeps_body: std::collections::HashSet<&MessageId> = keeps_body.iter().collect();
        docs.retain(|d| stored.contains(&d.message_id) && !keeps_body.contains(&d.message_id));
        if let Err(err) = index.add(&docs) {
            tracing::warn!(%err, "cannot index new messages");
        }
    }
    Ok(report)
}

/// The M0 thread grouping key (root of the `References` chain, else the
/// parent, else the message itself). The store threads by the full headers
/// ([`MessageMeta`]); this key only matters for rows without them.
fn thread_key(parsed: &ParsedMessage) -> Option<String> {
    parsed
        .references
        .first()
        .or_else(|| parsed.in_reply_to.first())
        .cloned()
        .or_else(|| parsed.headers.message_id.clone())
}

/// Fills in threading headers and sizes for messages stored before schema
/// v2 (parsed again from their blobs). Returns how many were updated.
pub(crate) fn backfill_meta(store: &Store, blobs: &BlobStore) -> nail_store::Result<usize> {
    let mut updated = 0;
    loop {
        let batch = store.messages_missing_meta(200)?;
        if batch.is_empty() {
            break;
        }
        let mut rows = Vec::with_capacity(batch.len());
        let mut stuck = 0;
        for (id, hash) in &batch {
            let message = store.get_message(id)?;
            let Ok(raw) = blobs.get(hash) else {
                stuck += 1;
                continue;
            };
            let parsed = nail_mime::parse_message(&raw);
            // The size is always set, so the row is not picked up again.
            rows.push(SyncedMessage {
                message,
                meta: Some(meta_of(&parsed, raw.len())),
            });
        }
        let written = store.merge_messages(&rows)?.written;
        updated += written;
        if written == 0 || stuck == batch.len() {
            break;
        }
    }
    Ok(updated)
}

/// Rebuilds the search index from every stored message's blob. Returns the
/// number of indexed messages.
pub(crate) fn reindex_all(
    store: &Store,
    blobs: &BlobStore,
    index: &SearchIndex,
) -> Result<usize, nail_search::Error> {
    index.clear()?;
    let ids = store.filter_message_ids(&MessageFilter::All)?;
    let mut count = 0;
    for chunk in ids.chunks(200) {
        let mut docs = Vec::with_capacity(chunk.len());
        for id in chunk {
            let Ok(message) = store.get_message(id) else {
                continue;
            };
            let Some(raw) = message.blob_hash.and_then(|h| blobs.get(&h).ok()) else {
                continue;
            };
            docs.push(IndexDocument::from_raw(
                id.clone(),
                message.summary.account_id,
                &raw,
            ));
        }
        count += docs.len();
        index.add(&docs)?;
    }
    index.commit()?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::{Account, AccountKind, FolderRole};

    fn setup() -> (Store, BlobStore, tempfile::TempDir, Folder) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let blobs = BlobStore::open(dir.path().join("blobs")).unwrap();
        store
            .upsert_account(&Account {
                id: "a".into(),
                name: "A".into(),
                email: "a@x".into(),
                kind: AccountKind::Imap,
            })
            .unwrap();
        let folder = Folder {
            id: ids::folder_id(&"a".into(), "INBOX"),
            account_id: "a".into(),
            parent_id: None,
            name: "INBOX".into(),
            path: "INBOX".into(),
            role: FolderRole::Inbox,
            unread: 0,
            total: 0,
        };
        store.upsert_folder(&folder).unwrap();
        (store, blobs, dir, folder)
    }

    fn mail(uid: u32, mid: &str, extra: &str) -> IncomingMessage {
        IncomingMessage {
            uid: Some(uid),
            raw: format!(
                "Message-ID: <{mid}>\r\n{extra}Subject: s{uid}\r\nFrom: x@y\r\nDate: Mon, 1 Jan 2024 10:00:00 +0000\r\n\r\nbody {uid} kangaroo"
            )
            .into_bytes(),
            keywords: vec!["$label1".into()],
            ..IncomingMessage::default()
        }
    }

    #[test]
    fn stores_threads_and_indexes() {
        let (store, blobs, _dir, folder) = setup();
        let index = SearchIndex::open_in_ram(nail_search::CommitPolicy::default()).unwrap();
        let report = ingest(
            &store,
            &blobs,
            Some(&index),
            &folder,
            7,
            &[mail(1, "a@x", ""), mail(2, "b@x", "In-Reply-To: <a@x>\r\n")],
        )
        .unwrap();
        assert_eq!(report.new.len(), 2);
        assert_eq!(report.stored, report.new);
        let first = store.get_message(&report.stored[0]).unwrap();
        assert_eq!(first.summary.tags, ["$label1"]);
        let meta = store.message_meta(&report.stored[1]).unwrap();
        assert_eq!(meta.in_reply_to, ["a@x"]);
        assert!(meta.size.unwrap() > 0);
        assert_eq!(
            store.thread_of(&report.stored[0]).unwrap(),
            store.thread_of(&report.stored[1]).unwrap()
        );
        index.commit().unwrap();
        assert_eq!(index.doc_count().unwrap(), 2);

        // Again: nothing new, same ids.
        let again = ingest(&store, &blobs, None, &folder, 7, &[mail(1, "a@x", "")]).unwrap();
        assert!(again.new.is_empty(), "{:?}", again.new);
        assert_eq!(again.stored, report.stored[..1]);
    }

    #[test]
    fn adopts_a_moved_row_without_uid() {
        let (store, blobs, _dir, folder) = setup();
        let first = ingest(&store, &blobs, None, &folder, 1, &[mail(1, "m@x", "")]).unwrap();
        let id = first.stored[0].clone();
        store.set_message_uid(&id, None).unwrap();
        let next = ingest(&store, &blobs, None, &folder, 1, &[mail(9, "m@x", "")]).unwrap();
        assert_eq!(next.stored, std::slice::from_ref(&id));
        assert!(next.new.is_empty(), "{:?}", next.new);
        assert_eq!(store.get_message(&id).unwrap().uid, Some(9));
    }

    #[test]
    fn prepared_batches_keep_their_order() {
        let (store, blobs, _dir, folder) = setup();
        let messages: Vec<IncomingMessage> = (1..=40)
            .map(|uid| mail(uid, &format!("m{uid}@x"), ""))
            .collect();
        let batch = prepare(&blobs, messages.clone()).unwrap();
        assert_eq!(batch.messages.len(), 40);
        for (prepared, message) in batch.messages.iter().zip(&messages) {
            assert_eq!(prepared.message.uid, message.uid);
            assert_eq!(prepared.blob_hash, BlobHash::of(&message.raw));
            assert!(blobs.contains(&prepared.blob_hash));
            assert_eq!(
                prepared.parsed.headers.message_id.as_deref(),
                Some(format!("m{}@x", message.uid.unwrap()).as_str())
            );
        }
        let report = ingest_prepared(&store, &blobs, None, &folder, 3, &batch).unwrap();
        assert_eq!(report.new.len(), 40);
        let uids: Vec<u32> = report
            .stored
            .iter()
            .map(|id| store.get_message(id).unwrap().uid.unwrap())
            .collect();
        assert_eq!(uids, (1..=40).collect::<Vec<_>>());
        assert!(prepare_threads(1) == 1 && prepare_threads(40) >= 1);
        assert!(prepare_threads(10_000) <= MAX_PREPARE_THREADS);
    }

    #[test]
    fn a_blob_removed_after_prepare_is_stored_again() {
        let (store, blobs, _dir, folder) = setup();
        let batch = prepare(&blobs, vec![mail(1, "gone@x", "")]).unwrap();
        // A garbage collection between prepare and the merge.
        let hash = batch.messages[0].blob_hash.clone();
        assert!(blobs.remove(&hash).unwrap());
        let report = ingest_prepared(&store, &blobs, None, &folder, 1, &batch).unwrap();
        assert_eq!(report.new.len(), 1);
        assert!(blobs.contains(&hash), "the blob was written again");
        let stored = store.get_message(&report.new[0]).unwrap();
        assert_eq!(stored.blob_hash, Some(hash));
    }

    #[test]
    fn existing_uids_and_moved_rows_keep_their_ids() {
        let (store, blobs, _dir, folder) = setup();
        let first = ingest(&store, &blobs, None, &folder, 5, &[mail(1, "a@x", "")]).unwrap();
        // The same UID again (a resync): same row, not new.
        let again = ingest(&store, &blobs, None, &folder, 5, &[mail(1, "a@x", "")]).unwrap();
        assert_eq!(again.stored, first.stored);
        assert!(again.new.is_empty(), "{:?}", again.new);
        // Two UID-less rows with one Message-ID: the oldest is adopted.
        let older = ingest(&store, &blobs, None, &folder, 5, &[mail(2, "dup@x", "")]).unwrap();
        let newer = ingest(
            &store,
            &blobs,
            None,
            &folder,
            5,
            &[IncomingMessage {
                raw: b"Message-ID: <dup@x>\r\nSubject: s\r\nFrom: x@y\r\nDate: Tue, 2 Jan 2024 10:00:00 +0000\r\n\r\nb"
                    .to_vec(),
                ..mail(3, "dup@x", "")
            }],
        )
        .unwrap();
        store.set_message_uid(&older.stored[0], None).unwrap();
        store.set_message_uid(&newer.stored[0], None).unwrap();
        let adopted = ingest(&store, &blobs, None, &folder, 5, &[mail(9, "dup@x", "")]).unwrap();
        assert_eq!(adopted.stored, older.stored);
        assert!(adopted.new.is_empty(), "{:?}", adopted.new);
        assert_eq!(store.get_message(&older.stored[0]).unwrap().uid, Some(9));
        assert_eq!(store.get_message(&newer.stored[0]).unwrap().uid, None);
    }

    #[test]
    fn reindexes_from_blobs() {
        let (store, blobs, _dir, folder) = setup();
        ingest(&store, &blobs, None, &folder, 1, &[mail(1, "m@x", "")]).unwrap();
        let index = SearchIndex::open_in_ram(nail_search::CommitPolicy::default()).unwrap();
        assert_eq!(reindex_all(&store, &blobs, &index).unwrap(), 1);
        assert_eq!(index.doc_count().unwrap(), 1);
        assert_eq!(backfill_meta(&store, &blobs).unwrap(), 0);
    }
}
