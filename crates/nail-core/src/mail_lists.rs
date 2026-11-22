//! The rows of a whole message list (R5, ADR 0235): what a
//! [`nail_api::MessageTarget::List`] stands for.
//!
//! The rows are listed with the very queries the message list pages with
//! (`ListThreads`, `ListMessages`, `Search` without the server part), page
//! by page, and each row becomes the target it stands for in the list (the
//! UI's `rowTarget`): a conversation row a conversation target with the
//! listed folder (none in a search), a message row a message target.

use nail_api::mail_actions::MailList;
use nail_api::{AccountId, ApiError, FolderId, MessageTarget, Response, SearchQuery};
use nail_store::ScopeFolder;

use crate::conversations::FolderFilter;
use crate::error::Result;
use crate::search::ServerSearchMode;
use crate::service::{MAX_PAGE_SIZE, Service};

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

/// The target a search hit's row stands for.
fn hit_row(message: &nail_api::MessageSummary, conversations: bool) -> MessageTarget {
    match (&message.thread_id, conversations) {
        (Some(thread), true) => MessageTarget::conversation(thread.clone(), None),
        _ => MessageTarget::message(message.id.clone()),
    }
}

impl Service {
    /// Every row of `list` as the target it stands for, in list order.
    pub(crate) async fn mail_list_rows(
        &self,
        list: &MailList,
        filter: Option<&FolderFilter>,
    ) -> Result<Vec<MessageTarget>> {
        let mut rows = Vec::new();
        let mut offset = 0_u32;
        loop {
            let (page, total) = match list {
                MailList::Conversations { folder_id } => {
                    let page = self
                        .list_conversations(folder_id, offset, MAX_PAGE_SIZE, filter)
                        .await?;
                    let rows = page
                        .threads
                        .into_iter()
                        .map(|t| MessageTarget::conversation(t.id, Some(folder_id.clone())))
                        .collect::<Vec<_>>();
                    (rows, page.total)
                }
                MailList::Messages { folder_id } => {
                    let page = self.list_messages(folder_id, offset, MAX_PAGE_SIZE).await?;
                    let rows = page
                        .messages
                        .into_iter()
                        .map(|m| MessageTarget::message(m.id))
                        .collect::<Vec<_>>();
                    (rows, page.total)
                }
                MailList::Search {
                    query,
                    conversations,
                } => {
                    let page = SearchQuery {
                        offset,
                        limit: MAX_PAGE_SIZE,
                        ..query.clone()
                    };
                    match self.search_with(page, ServerSearchMode::Off).await? {
                        Response::SearchResults { total, hits, .. } => (
                            hits.iter()
                                .map(|h| hit_row(&h.message, *conversations))
                                .collect(),
                            total,
                        ),
                        Response::InvalidQuery { error } => {
                            return Err(ApiError::invalid_request(format!(
                                "invalid query: {}",
                                error.message
                            ))
                            .into());
                        }
                        other => {
                            return Err(ApiError::internal(format!(
                                "unexpected search answer {other:?}"
                            ))
                            .into());
                        }
                    }
                }
            };
            let got = u32::try_from(page.len()).unwrap_or(u32::MAX);
            rows.extend(page);
            offset = offset.saturating_add(got);
            if got == 0 || offset >= total {
                return Ok(rows);
            }
        }
    }
}
