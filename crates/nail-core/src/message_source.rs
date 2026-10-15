//! "Show source" (ADR 0194): `GetMessageSource` answers the message as it
//! was received and decoded for reading (`nail_mime::original_source` /
//! `decoded_source`). An end-to-end encrypted message is decrypted in memory
//! for the decoded view only (never persisted), as the reader does.

use nail_api::message_source::{MAX_MESSAGE_SOURCE_BYTES, MessageSource};
use nail_api::{MessageId, Response};

use crate::attachments::downloaded_raw;
use crate::error::Result;
use crate::service::Service;

/// `GetMessageSource`.
pub(crate) async fn handle(service: &Service, message_id: MessageId) -> Result<Response> {
    let raw = downloaded_raw(service, message_id).await?;
    let service = service.clone();
    let source = tokio::task::spawn_blocking(move || {
        let (original, original_truncated) =
            nail_mime::original_source(&raw, MAX_MESSAGE_SOURCE_BYTES);
        let size = raw.len() as u64;
        let (readable, status) = service.readable_raw(raw);
        let (decoded, decoded_truncated) =
            nail_mime::decoded_source(&readable, MAX_MESSAGE_SOURCE_BYTES);
        MessageSource {
            size,
            original,
            original_truncated,
            decoded,
            decoded_truncated,
            encrypted: status.is_some_and(|s| s.is_encrypted()),
        }
    })
    .await?;
    Ok(Response::MessageSource {
        source: Box::new(source),
    })
}
