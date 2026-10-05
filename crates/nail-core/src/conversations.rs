//! Conversations (P2, ADR 0148): the slot of WP-core.
//!
//! `GetConversation` answers the conversation of a thread as a list row
//! shows it: the thread's messages in the listed folder plus the user's own
//! messages from the accounts' Sent folders (`from_me`), oldest first,
//! never drafts; without a folder, every folder but
//! Drafts, Trash and Junk. `ListThreads` counts the same set, and
//! resolves "me" in the participants from the accounts' identities.
//!
//! Contract stub: until WP-core replaces it, the answer is `GetThread`'s
//! (every stored copy across folders, nothing marked `from_me`), so the UI
//! can be built against the request on the real core.

use nail_api::{ConversationMessage, FolderId, ThreadId, ThreadSummary};

use crate::error::Result;
use crate::service::Service;

impl Service {
    /// `GetConversation`: the conversation's row and its messages, oldest
    /// first.
    pub(crate) async fn get_conversation(
        &self,
        thread: &ThreadId,
        folder: Option<&FolderId>,
    ) -> Result<(ThreadSummary, Vec<ConversationMessage>)> {
        if let Some(folder) = folder {
            // Unknown folders are an error, as for ListThreads.
            if !self.resolve_folder(folder).await?.is_virtual {
                let id = folder.clone();
                self.inner.store.call(move |s| s.get_folder(&id)).await?;
            }
        }
        let (summary, messages) = self.get_thread(thread).await?;
        Ok((summary, stub_messages(messages)))
    }
}

/// The contract stub's messages: every message as it is, none marked as
/// the user's.
fn stub_messages(messages: Vec<nail_api::MessageSummary>) -> Vec<ConversationMessage> {
    messages
        .into_iter()
        .map(|message| ConversationMessage {
            message,
            from_me: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use nail_api::{Flags, MessageSummary};

    use super::stub_messages;

    #[test]
    fn the_stub_keeps_order_and_marks_nothing_as_mine() {
        let message = |id: &str| MessageSummary {
            id: id.into(),
            account_id: "a".into(),
            folder_id: "f".into(),
            subject: String::new(),
            from: None,
            to: Vec::new(),
            date: String::new(),
            flags: Flags::default(),
            has_attachments: false,
            preview: String::new(),
            thread_id: None,
            tags: Vec::new(),
            duplicates: Vec::new(),
            headers_only: false,
            gmail: None,
            exchange: None,
        };
        let out = stub_messages(vec![message("1"), message("2")]);
        assert_eq!(
            out.iter()
                .map(|m| m.message.id.as_str())
                .collect::<Vec<_>>(),
            ["1", "2"]
        );
        assert!(out.iter().all(|m| !m.from_me));
    }
}
