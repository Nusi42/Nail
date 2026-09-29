//! Launch requests (ADR 0041): the desktop shell hands the core what
//! Windows asked the app to open ([`Service::push_launch_requests`]); the
//! UI takes them (`TakeLaunchRequests`) and imports `.eml` files
//! (`ImportMessageFile`) into a local "Opened files" account to show them.

use nail_api::launch::{LaunchRequest, OPENED_FILES_ACCOUNT_ID};
use nail_api::{
    Account, AccountId, AccountKind, ApiError, Event, Folder, FolderRole, MessageId, Request,
    Response,
};

use crate::error::Result;
use crate::ingest::{IncomingMessage, ingest};
use crate::service::{Service, lock};

/// Largest `.eml` file imported (like the largest attachment total).
pub const MAX_MESSAGE_FILE: u64 = 50 * 1024 * 1024;

impl Service {
    /// Queues launch requests for the UI and tells it
    /// ([`Event::LaunchRequested`]).
    pub fn push_launch_requests(&self, requests: Vec<LaunchRequest>) {
        if requests.is_empty() {
            return;
        }
        lock(&self.inner.launches).extend(requests);
        self.broadcast(Event::LaunchRequested);
    }
}

pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::TakeLaunchRequests => Ok(Response::LaunchRequests {
            requests: std::mem::take(&mut *lock(&service.inner.launches)),
        }),
        Request::ImportMessageFile { path } => {
            let summary = service
                .blocking(move |s| import_message_file(s, std::path::Path::new(&path)))
                .await?;
            service.broadcast(Event::AccountsChanged);
            service.broadcast(Event::MailChanged {
                folder_ids: vec![summary.folder_id.clone()],
            });
            Ok(Response::MessageImported {
                message: Box::new(summary),
            })
        }
        other => Err(ApiError::internal(format!(
            "{} is not a launch request",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

/// Reads `path` and stores it in the "Opened files" account (blocking).
/// Importing the same file again finds the same message.
fn import_message_file(
    service: &Service,
    path: &std::path::Path,
) -> Result<nail_api::MessageSummary> {
    let size = std::fs::metadata(path)
        .map_err(|err| ApiError::not_found(format!("{}: {err}", path.display())))?
        .len();
    if size == 0 || size > MAX_MESSAGE_FILE {
        return Err(ApiError::invalid_request(format!(
            "{} is empty or larger than {MAX_MESSAGE_FILE} bytes",
            path.display()
        ))
        .into());
    }
    let raw = std::fs::read(path)
        .map_err(|err| ApiError::not_found(format!("{}: {err}", path.display())))?;
    let parsed = nail_mime::parse_message(&raw);
    if parsed.headers.from.is_empty() && parsed.headers.subject.is_empty() && parsed.text.is_empty()
    {
        return Err(ApiError::invalid_request(format!(
            "{} is not an email message",
            path.display()
        ))
        .into());
    }
    let store = service.store();
    let locale = service.inner.locale.as_deref();
    let account = Account {
        id: AccountId::new(OPENED_FILES_ACCOUNT_ID),
        name: crate::strings::message(locale, "opened-files-account"),
        email: String::new(),
        kind: AccountKind::Local,
    };
    store.upsert_account(&account)?;
    let folder = Folder {
        id: crate::ids::folder_id(&account.id, "INBOX"),
        account_id: account.id.clone(),
        parent_id: None,
        name: crate::strings::message(locale, "opened-files-folder"),
        path: "INBOX".to_owned(),
        role: FolderRole::Inbox,
        unread: 0,
        total: 0,
    };
    store.upsert_folder(&folder)?;
    let content = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&raw));
    let id: MessageId = crate::ids::local_message_id(&folder.id, &content);
    let index = service.inner.search.get();
    ingest(
        store,
        service.blobs(),
        Some(&index),
        &folder,
        1,
        &[IncomingMessage {
            id: Some(id.clone()),
            raw,
            ..IncomingMessage::default()
        }],
    )?;
    Ok(store.get_message(&id)?.summary)
}
