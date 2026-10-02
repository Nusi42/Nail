//! The MCP prompts (M4a, ADR 0079; design.md, "MCP server" → Prompts):
//! `summarize_thread`, `triage_folder`, `draft_reply`, `prepare_meeting`.
//!
//! A prompt is a message template the client fills in and hands to its
//! model. Nail builds it from the user's data (always marked as untrusted
//! data, like tool results) and a template text from the Fluent catalog
//! `core-mcp.ftl` (`mcp-prompt-<name>-description`,
//! `mcp-prompt-<name>-instructions`), rendered in the core's locale.
//! Getting a prompt needs the `read` scope plus the scopes of what it
//! reads; it writes nothing (the model may then call write tools, which
//! are confirmed as usual).

use serde::{Deserialize, Serialize};

use crate::mcp::McpScope;

/// One prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpPrompt {
    /// Summarize a thread: its messages in order, with senders and dates.
    SummarizeThread,
    /// Triage a folder: its newest unread messages with suggested actions
    /// (archive, flag, reply, tag) for the model to propose.
    TriageFolder,
    /// Draft a reply to a message, with the thread for context; the model
    /// writes the draft with `create_draft`.
    DraftReply,
    /// Prepare a meeting: the event, its attendees (with contact details)
    /// and related mail.
    PrepareMeeting,
}

impl McpPrompt {
    /// Every prompt.
    pub const ALL: [McpPrompt; 4] = [
        Self::SummarizeThread,
        Self::TriageFolder,
        Self::DraftReply,
        Self::PrepareMeeting,
    ];

    /// The prompt's name (`summarize_thread`).
    pub fn name(self) -> &'static str {
        match self {
            Self::SummarizeThread => "summarize_thread",
            Self::TriageFolder => "triage_folder",
            Self::DraftReply => "draft_reply",
            Self::PrepareMeeting => "prepare_meeting",
        }
    }

    /// Parses [`Self::name`].
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The scopes a client needs to get it.
    pub fn scopes(self) -> &'static [McpScope] {
        &[McpScope::Read]
    }

    /// Its arguments: (name, required).
    pub fn arguments(self) -> &'static [(&'static str, bool)] {
        match self {
            Self::SummarizeThread => &[("thread_id", true)],
            Self::TriageFolder => &[("folder_id", true), ("limit", false)],
            Self::DraftReply => &[
                ("message_id", true),
                ("instructions", false),
                ("reply_all", false),
            ],
            Self::PrepareMeeting => &[("event_id", true), ("occurrence", false)],
        }
    }

    /// Fluent id of the description clients list
    /// (`mcp-prompt-<name>-description`, `_` as `-`).
    pub fn description_id(self) -> String {
        format!("mcp-prompt-{}-description", self.name().replace('_', "-"))
    }

    /// Fluent id of the instructions that open the prompt
    /// (`mcp-prompt-<name>-instructions`).
    pub fn instructions_id(self) -> String {
        format!("mcp-prompt-{}-instructions", self.name().replace('_', "-"))
    }
}

/// Most messages `triage_folder` includes by default.
pub const TRIAGE_DEFAULT_LIMIT: u32 = 25;
/// Most messages `triage_folder` includes at all.
pub const TRIAGE_MAX_LIMIT: u32 = 100;
/// Most messages of a thread `summarize_thread` / `draft_reply` include
/// (the newest ones).
pub const PROMPT_MAX_THREAD_MESSAGES: u32 = 50;
/// Most related messages `prepare_meeting` includes.
pub const MEETING_MAX_RELATED: u32 = 20;

/// `summarize_thread` arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct SummarizeThreadArgs {
    /// The thread.
    pub thread_id: String,
}

/// `triage_folder` arguments (MCP prompt arguments are strings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct TriageFolderArgs {
    /// The folder.
    pub folder_id: String,
    /// How many unread messages (default 25, at most 100), as a string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
}

/// `draft_reply` arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct DraftReplyArgs {
    /// The message to answer.
    pub message_id: String,
    /// What the reply should say (the user's words).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// `"true"` to reply to all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_all: Option<String>,
}

/// `prepare_meeting` arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct PrepareMeetingArgs {
    /// The event.
    pub event_id: String,
    /// For recurring events: the occurrence (RFC 3339 original start).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prompts() {
        assert_eq!(McpPrompt::ALL.len(), 4);
        for p in McpPrompt::ALL {
            assert_eq!(McpPrompt::parse(p.name()), Some(p));
            assert_eq!(serde_json::to_value(p).unwrap(), json!(p.name()));
            assert!(crate::localized::is_message_id(&p.description_id()));
            assert!(crate::localized::is_message_id(&p.instructions_id()));
            assert!(p.arguments().iter().any(|(_, required)| *required));
            assert_eq!(p.scopes(), [McpScope::Read]);
        }
        assert_eq!(
            McpPrompt::DraftReply.instructions_id(),
            "mcp-prompt-draft-reply-instructions"
        );
        assert_eq!(McpPrompt::parse("nope"), None);
        let args: TriageFolderArgs =
            serde_json::from_value(json!({"folder_id": "f1", "limit": "10"})).unwrap();
        assert_eq!(args.limit.as_deref(), Some("10"));
        let reply: DraftReplyArgs = serde_json::from_value(json!({"message_id": "m1"})).unwrap();
        assert_eq!(reply.instructions, None);
    }
}
