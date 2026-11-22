//! The rows of a whole message list (R5, ADR 0235): what a
//! [`nail_api::MessageTarget::List`] stands for.
//!
//! The rows are listed with the very queries the message list pages with
//! (`ListThreads`, `ListMessages`, `Search` without the server part), but
//! all at once in one read of the store, so rows removed or added while
//! the list is read never shift a page (a row skipped or listed twice).
//! Each row becomes the target it stands for in the list
//! ([`MailList::row_target`], the UI's `rowTarget`).

use nail_api::mail_actions::MailList;
use nail_api::{AccountId, ApiError, FolderId, MessageTarget};
use nail_store::ScopeFolder;

use crate::conversations::FolderFilter;
use crate::error::Result;
use crate::service::Service;

/// Whether a message in `folder` of `account` is within `filter` (a remote
/// session's grant; `None` = everything).
pub(crate) fn within(
    filter: Option<&FolderFilter>,
    folder: &FolderId,
    account: &AccountId,
) -> bool {
    filter.is_none_or(|keep| {
        keep(&ScopeFolder {
            id: folder.clone(),
            account_id: account.clone(),
        })
    })
}

impl Service {
    /// Every row of `list` as the target it stands for, in list order.
    pub(crate) async fn mail_list_rows(
        &self,
        list: &MailList,
        filter: Option<&FolderFilter>,
    ) -> Result<Vec<MessageTarget>> {
        Ok(match list {
            MailList::Conversations { folder_id } => self
                .list_conversations(folder_id, 0, u32::MAX, filter)
                .await?
                .threads
                .into_iter()
                .map(|t| list.row_target(&t.latest_message_id, Some(&t.id)))
                .collect(),
            MailList::Messages { folder_id } => self
                .list_messages(folder_id, 0, u32::MAX)
                .await?
                .messages
                .into_iter()
                .map(|m| list.row_target(&m.id, m.thread_id.as_ref()))
                .collect(),
            MailList::Search { query, .. } => match self.local_hits(query).await? {
                Ok(hits) => hits
                    .iter()
                    .map(|(message, thread)| list.row_target(message, thread.as_ref()))
                    .collect(),
                Err(error) => {
                    return Err(ApiError::invalid_request(format!(
                        "invalid query: {}",
                        error.message
                    ))
                    .into());
                }
            },
        })
    }
}
