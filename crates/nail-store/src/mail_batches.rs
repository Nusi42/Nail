//! Mail action batches with one undo (P3, ADR 0164; slot of
//! WP-actions-core).
//!
//! `ApplyMailAction` records its queued operations and their inverses in
//! `mail_batches` / `mail_batch_ops` (schema v17) in the same transaction
//! as the local change and the queue entries, like a rule run's actions
//! (`rules.rs`). `UndoMailAction` queues the inverses newest first in one
//! transaction and marks the batch `undone`; an inverse that no longer
//! applies (the message moved or was deleted since) is skipped. Batches
//! older than `nail_api::mail_actions::MAIL_UNDO_SECS`, and all but the
//! newest `MAIL_UNDO_KEEP`, are purged on write.
