//! Attachments of received mail (M5a WP155): the decoded content for the
//! web UI's download (`GetMessageAttachment`) and saving it to a file on the
//! desktop (`SaveAttachment`, after the UI asked for the path with
//! `PickPath`).
//!
//! Both read the message the way the reader shows it: a body that was
//! synced headers-only is fetched first, an end-to-end encrypted message is
//! decrypted in memory (never persisted), and the index is the one of
//! `MessageBody.attachments`.

use std::path::PathBuf;

use base64::Engine as _;
use nail_api::{
    ApiError, ErrorCode, LocalizedMessage, MAX_ATTACHMENT_DOWNLOAD, MessageId, Request, Response,
};

use crate::error::{CoreError, Result};
use crate::service::Service;

/// `GetMessageAttachment`, `SaveAttachment`.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetMessageAttachment { message_id, index } => {
            let content = attachment(service, message_id, index).await?;
            if content.data.len() > MAX_ATTACHMENT_DOWNLOAD {
                return Err(ApiError::new(
                    ErrorCode::TooLarge,
                    format!(
                        "attachment {index} has {} bytes; downloads are limited to {MAX_ATTACHMENT_DOWNLOAD}",
                        content.data.len()
                    ),
                )
                .with_localized(LocalizedMessage::new("attachment-too-large"))
                .into());
            }
            Ok(Response::AttachmentData {
                mime: content.info.mime,
                data: base64::engine::general_purpose::STANDARD.encode(&content.data),
            })
        }
        Request::SaveAttachment {
            message_id,
            index,
            path,
            overwrite,
        } => {
            let target = PathBuf::from(&path);
            if !target.is_absolute() {
                return Err(
                    ApiError::invalid_request(format!("{path} is not an absolute path"))
                        .with_localized(LocalizedMessage::new("attachment-path-not-absolute"))
                        .into(),
                );
            }
            let content = attachment(service, message_id, index).await?;
            tokio::task::spawn_blocking(move || {
                nail_portability::files::write_file(&target, &content.data, overwrite)
                    .map_err(|e| CoreError::from(e.to_api_error()))
            })
            .await??;
            Ok(Response::Done)
        }
        other => Err(ApiError::internal(format!(
            "not an attachment request: {}",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

/// The decoded attachment `index` of a message, as the reader numbers it.
async fn attachment(
    service: &Service,
    id: MessageId,
    index: u32,
) -> Result<nail_mime::AttachmentContent> {
    service.inner.vault.check()?;
    // A body synced headers-only is fetched first (like `GetMessage`).
    let state = {
        let id = id.clone();
        service.store().call(move |s| s.body_state(&id)).await
    };
    if matches!(state, Ok(nail_api::sync::BodyState::HeadersOnly)) {
        service.fetch_bodies(vec![id.clone()]).await?;
    }
    let service = service.clone();
    tokio::task::spawn_blocking(move || -> Result<nail_mime::AttachmentContent> {
        let stored = service.store().get_message(&id)?;
        let hash = stored.blob_hash.ok_or_else(|| {
            CoreError::from(ApiError::new(
                ErrorCode::Unavailable,
                format!("the body of message {id} is not downloaded yet"),
            ))
        })?;
        let (raw, _) = service.readable_raw(service.blobs().get(&hash)?);
        nail_mime::attachment_content(&raw, index).ok_or_else(|| {
            CoreError::from(ApiError::not_found(format!(
                "message {id} has no attachment {index}"
            )))
        })
    })
    .await?
}
