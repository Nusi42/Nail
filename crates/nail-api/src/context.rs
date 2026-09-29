//! UI context: what the user has in front of them (design.md, "MCP
//! server", `nail://ui/context`).
//!
//! The UI reports its context with [`crate::Request::ReportUiContext`]
//! whenever the open message, the selection or the open draft changes; the
//! core keeps the latest report (per connected UI, the most recent wins) and
//! serves it to MCP clients holding the `context` grant. Nothing here is
//! persisted.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{AccountId, DraftId, FolderId, MessageId, ThreadId};

/// The UI's current context.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct UiContext {
    /// The active account (real or virtual).
    pub account_id: Option<AccountId>,
    /// The active folder.
    pub folder_id: Option<FolderId>,
    /// The message open in the reader.
    pub message_id: Option<MessageId>,
    /// Its thread.
    pub thread_id: Option<ThreadId>,
    /// Selected messages in the list (may include the open one).
    pub selected_message_ids: Vec<MessageId>,
    /// Text selected in the reader, if any.
    pub selected_text: Option<String>,
    /// The draft open in the composer (saved at least once).
    pub draft_id: Option<DraftId>,
    /// Cursor position in the draft's source (UTF-16 offset).
    pub draft_cursor: Option<u32>,
}

impl UiContext {
    /// Whether nothing is open or selected.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn partial_reports_parse() {
        let ctx: UiContext = serde_json::from_value(json!({"message_id": "m1"})).unwrap();
        assert_eq!(ctx.message_id, Some(MessageId::from("m1")));
        assert!(ctx.selected_message_ids.is_empty());
        assert!(!ctx.is_empty());
        assert!(UiContext::default().is_empty());
    }
}
