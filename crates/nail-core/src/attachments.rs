//! Attachments of received mail (M5a WP155): the decoded content for the
//! web UI's download (`GetMessageAttachment`) and saving it to a file on the
//! desktop (`SaveAttachment`, after the UI asked for the path with
//! `PickPath`).
//!
//! Both read the message the way the reader shows it: a body that was
//! synced headers-only is fetched first, an end-to-end encrypted message is
//! decrypted in memory (never persisted), and the index is the one of
//! `MessageBody.attachments`.
//!
//! `OpenAttachment` (ADR 0211) writes the attachment into a private folder
//! of the data directory and asks the desktop host to open that file; see
//! [`OpenedFiles`].

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use nail_api::{
    ApiError, ErrorCode, LocalizedMessage, MAX_ATTACHMENT_DOWNLOAD, MessageId, Request, Response,
};

use crate::error::{CoreError, Result};
use crate::service::Service;

/// `GetMessageAttachment`, `SaveAttachment`, `OpenAttachment`.
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
                return Err(ApiError::invalid_localized(
                    format!("{path} is not an absolute path"),
                    "attachment-path-not-absolute",
                )
                .into());
            }
            let content = attachment(service, message_id, index).await?;
            tokio::task::spawn_blocking(move || {
                nail_portability::files::write_file(&target, &content.data, overwrite)
                    .map_err(|e| CoreError::from(e.to_api_error()))
            })
            .await??;
            Ok(Response::Done)
        }
        Request::OpenAttachment { message_id, index } => {
            let host = service.inner.desktop.as_ref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Unsupported,
                    "attachments are opened by the desktop app only",
                )
            })?;
            let content = attachment(service, message_id, index).await?;
            let opened = service.inner.opened_files.clone();
            let name = content.info.filename.clone();
            let path =
                tokio::task::spawn_blocking(move || opened.stage(name.as_deref(), &content.data))
                    .await??;
            host.open_file(&path).map_err(|e| {
                tracing::warn!(error = %e, "cannot open an attachment");
                CoreError::from(
                    ApiError::internal(format!("cannot open the attachment: {e}"))
                        .with_localized(LocalizedMessage::new("attachment-open-failed")),
                )
            })?;
            Ok(Response::Done)
        }
        other => Err(ApiError::internal(format!(
            "not an attachment request: {}",
            other.wire_type()
        ))
        .into()),
    }
}

/// Name of the data directory's folder for opened attachments.
const OPENED_DIR: &str = "opened-attachments";

/// How long an opened attachment's folder stays before the next open
/// removes it.
const OPENED_KEEP: Duration = Duration::from_hours(24);

/// Longest file name written for an opened attachment (characters).
const OPENED_NAME_CHARS: usize = 120;

/// The file name used for an attachment without one.
const OPENED_FALLBACK_NAME: &str = "attachment";

/// The private folder of attachments being opened (ADR 0211):
/// `<data dir>/opened-attachments/<unique>/<sanitized name>`. Each open gets
/// a subfolder of its own (two attachments with one name never clash and a
/// handler's side files stay apart); the folders are readable by the user
/// only on Unix, and live in the user's data directory elsewhere. All of it
/// goes at startup and at shutdown; every open also prunes folders older
/// than [`OPENED_KEEP`]. A viewer that still holds a file keeps it (the
/// removal of a file in use fails and is ignored).
#[derive(Debug)]
pub(crate) struct OpenedFiles {
    root: Option<PathBuf>,
    counter: AtomicU64,
}

impl OpenedFiles {
    /// The folder under `data_dir` (`None` = no data directory: nothing can
    /// be opened). Removes what a previous run left.
    pub(crate) fn new(data_dir: Option<PathBuf>) -> Self {
        let opened = Self {
            root: data_dir.map(|d| d.join(OPENED_DIR)),
            counter: AtomicU64::new(0),
        };
        opened.clear();
        opened
    }

    /// Removes every folder (best effort).
    pub(crate) fn clear(&self) {
        if let Some(root) = &self.root {
            drop(std::fs::remove_dir_all(root));
        }
    }

    /// Removes the folders older than `keep` (best effort).
    fn prune(root: &Path, keep: Duration) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age > keep);
            if old {
                drop(std::fs::remove_dir_all(entry.path()));
            }
        }
    }

    /// Writes `data` as `name` into a new subfolder; the file's path.
    pub(crate) fn stage(&self, name: Option<&str>, data: &[u8]) -> Result<PathBuf> {
        let root = self.root.as_ref().ok_or_else(|| {
            CoreError::from(ApiError::new(
                ErrorCode::Unsupported,
                "no data directory to open attachments from",
            ))
        })?;
        create_private_dir(root, true)?;
        Self::prune(root, OPENED_KEEP);
        let unique = format!(
            "{:x}-{:x}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
            self.counter.fetch_add(1, Ordering::Relaxed)
        );
        let folder = root.join(unique);
        create_private_dir(&folder, false)?;
        let file_name = nail_portability::folder::safe_file_name(
            name.filter(|n| !n.trim().is_empty())
                .unwrap_or(OPENED_FALLBACK_NAME),
            OPENED_NAME_CHARS,
        );
        let path = folder.join(file_name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(data)?;
        file.sync_all()?;
        Ok(path)
    }
}

/// Creates `dir` (and, with `parents`, its parents) readable by the user
/// only on Unix.
fn create_private_dir(dir: &Path, parents: bool) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(parents);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// The decoded attachment `index` of a message, as the reader numbers it.
async fn attachment(
    service: &Service,
    id: MessageId,
    index: u32,
) -> Result<nail_mime::AttachmentContent> {
    let raw = downloaded_raw(service, id.clone()).await?;
    let service = service.clone();
    tokio::task::spawn_blocking(move || -> Result<nail_mime::AttachmentContent> {
        let (raw, _) = service.readable_raw(raw);
        nail_mime::attachment_content(&raw, index).ok_or_else(|| {
            CoreError::from(ApiError::not_found(format!(
                "message {id} has no attachment {index}"
            )))
        })
    })
    .await?
}

/// A message's raw bytes as stored (encrypted mail stays encrypted). A
/// body synced headers-only is fetched first (like `GetMessage`).
pub(crate) async fn downloaded_raw(service: &Service, id: MessageId) -> Result<Vec<u8>> {
    service.inner.vault.check()?;
    let state = {
        let id = id.clone();
        service.store().call(move |s| s.body_state(&id)).await
    };
    if matches!(state, Ok(nail_api::sync::BodyState::HeadersOnly)) {
        service.fetch_bodies(vec![id.clone()]).await?;
    }
    let service = service.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let stored = service.store().get_message(&id)?;
        let hash = stored.blob_hash.ok_or_else(|| {
            CoreError::from(ApiError::new(
                ErrorCode::Unavailable,
                format!("the body of message {id} is not downloaded yet"),
            ))
        })?;
        Ok(service.blobs().get(&hash)?)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_files_get_a_folder_each_and_a_safe_name() {
        let dir = tempfile::tempdir().unwrap();
        let opened = OpenedFiles::new(Some(dir.path().to_path_buf()));
        let a = opened.stage(Some("../evil:name.pdf"), b"a").unwrap();
        let b = opened.stage(Some("../evil:name.pdf"), b"b").unwrap();
        let c = opened.stage(None, b"c").unwrap();
        let root = dir.path().join(OPENED_DIR);
        for path in [&a, &b, &c] {
            assert_eq!(path.parent().unwrap().parent().unwrap(), root, "{path:?}");
        }
        assert_ne!(a.parent(), b.parent());
        assert_eq!(a.file_name().unwrap(), ".._evil_name.pdf");
        assert_eq!(c.file_name().unwrap(), OPENED_FALLBACK_NAME);
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
    }

    #[test]
    fn pruning_removes_old_folders_and_clearing_removes_all() {
        let dir = tempfile::tempdir().unwrap();
        let opened = OpenedFiles::new(Some(dir.path().to_path_buf()));
        let file = opened.stage(Some("x.txt"), b"x").unwrap();
        let root = dir.path().join(OPENED_DIR);
        OpenedFiles::prune(&root, Duration::from_hours(1));
        assert!(file.exists());
        std::thread::sleep(Duration::from_millis(20));
        OpenedFiles::prune(&root, Duration::from_millis(1));
        assert!(!file.exists());
        let again = opened.stage(Some("y.txt"), b"y").unwrap();
        opened.clear();
        assert!(!again.exists() && !root.exists());
    }

    #[test]
    fn without_a_data_directory_nothing_is_staged() {
        assert!(OpenedFiles::new(None).stage(Some("x.txt"), b"x").is_err());
    }
}
