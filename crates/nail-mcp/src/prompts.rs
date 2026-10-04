//! The MCP prompts (M4a, ADR 0079; design.md, "MCP server" → Prompts):
//! `summarize_thread`, `triage_folder`, `draft_reply`, `prepare_meeting`.
//!
//! A prompt is two user messages: the instructions, rendered from
//! `core-mcp.ftl` in the core's locale (`mcp-prompt-<name>-instructions`,
//! plus the user's own wishes for `draft_reply`), and the data as an
//! embedded `nail://` resource in the untrusted-data envelope
//! ([`crate::content`]). Getting a prompt needs the `read` scope and only
//! shows what the grant allows (accounts, folders, encrypted content); it
//! changes nothing and is audited like a read.

use std::sync::Arc;

use nail_api::mcp::prompts::{
    DraftReplyArgs, MEETING_MAX_RELATED, McpPrompt, PROMPT_MAX_THREAD_MESSAGES, PrepareMeetingArgs,
    SummarizeThreadArgs, TRIAGE_DEFAULT_LIMIT, TRIAGE_MAX_LIMIT, TriageFolderArgs,
};
use nail_api::mcp::{McpCallKind, McpClient, McpOutcome, McpScope};
use nail_api::search::SearchSort;
use nail_api::{
    ApiError, EventId, FolderId, LocalizedMessage, MessageId, MessageSummary, Request, Response,
    SearchQuery, ThreadId,
};
use rmcp::ErrorData;
use rmcp::model::{
    ContentBlock, GetPromptRequestParams, GetPromptResult, Prompt, PromptArgument, PromptMessage,
    Role,
};
use serde_json::{Value, json};

use crate::access::Access;
use crate::content::untrusted_json_resource;
use crate::data::{McpData, SearchOutcome};
use crate::server::{Failure, Shared, summary_json};
use crate::uri::MAX_PAGE;

/// Messages `triage_folder` looks at, at most, to find unread ones.
const TRIAGE_SCAN: u32 = 1000;
/// Attendees `prepare_meeting` looks up as contacts, at most.
const MEETING_MAX_ATTENDEES: usize = 20;

/// English descriptions of the arguments (agents read them).
fn argument_description(prompt: McpPrompt, name: &str) -> &'static str {
    match (prompt, name) {
        (_, "thread_id") => "Thread id (a message's thread_id).",
        (_, "folder_id") => "Folder id (from list_folders).",
        (_, "limit") => "How many unread messages (default 25, at most 100).",
        (_, "message_id") => "The message to answer.",
        (_, "instructions") => "What the reply should say, in the user's words.",
        (_, "reply_all") => "\"true\" to reply to all recipients.",
        (_, "event_id") => "Event id (from list_events).",
        (_, "occurrence") => "For recurring events: the occurrence's original start (RFC 3339).",
        _ => "",
    }
}

/// The prompt list, with descriptions in the core's locale.
pub(crate) fn definitions<D: McpData>(data: &D) -> Vec<Prompt> {
    McpPrompt::ALL
        .into_iter()
        .map(|prompt| {
            let arguments = prompt
                .arguments()
                .iter()
                .map(|(name, required)| {
                    PromptArgument::new(*name)
                        .with_description(argument_description(prompt, name))
                        .with_required(*required)
                })
                .collect();
            Prompt::new(
                prompt.name(),
                Some(data.render(&LocalizedMessage::new(prompt.description_id()))),
                Some(arguments),
            )
        })
        .collect()
}

/// Prompt arguments as strings (clients may send numbers or booleans).
fn string_arguments(request: &GetPromptRequestParams) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in request.arguments.iter().flatten() {
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Null => continue,
            other => other.to_string(),
        };
        out.insert(key.clone(), Value::String(text));
    }
    Value::Object(out)
}

fn arguments<T: for<'de> serde::Deserialize<'de>>(value: Value) -> Result<T, ErrorData> {
    serde_json::from_value(value)
        .map_err(|e| ErrorData::invalid_params(format!("invalid arguments: {e}"), None))
}

/// Gets a prompt for `client`, audited.
pub(crate) async fn get<D: McpData>(
    shared: &Arc<Shared<D>>,
    client: &McpClient,
    request: GetPromptRequestParams,
) -> Result<GetPromptResult, ErrorData> {
    let name = crate::http::audited_path(&request.name);
    let Some(prompt) = McpPrompt::parse(&request.name) else {
        let err = ErrorData::invalid_params(format!("unknown prompt {name}"), None);
        shared
            .audit(
                Some(client),
                McpCallKind::Prompt,
                &name,
                McpOutcome::Error,
                Some(err.message.to_string()),
            )
            .await;
        return Err(err);
    };
    let args = string_arguments(&request);
    let built = if let Err(limited) = shared.count_request(client) {
        Err(limited)
    } else {
        let access = shared.access(client).await;
        match prompt {
            McpPrompt::SummarizeThread => summarize_thread(shared, &access, arguments(args)?).await,
            McpPrompt::TriageFolder => triage_folder(shared, &access, arguments(args)?).await,
            McpPrompt::DraftReply => draft_reply(shared, &access, arguments(args)?).await,
            McpPrompt::PrepareMeeting => prepare_meeting(shared, &access, arguments(args)?).await,
        }
    };
    let (texts, uri, data) = shared
        .audited(client, McpCallKind::Prompt, prompt.name(), built)
        .await
        .map_err(|f| f.to_error())?;
    let mut messages: Vec<PromptMessage> = texts
        .into_iter()
        .map(|t| PromptMessage::new_text(Role::User, shared.data.render(&t)))
        .collect();
    messages.push(PromptMessage::new(
        Role::User,
        ContentBlock::resource(untrusted_json_resource(&uri, data)),
    ));
    Ok(GetPromptResult::new(messages).with_description(
        shared
            .data
            .render(&LocalizedMessage::new(prompt.description_id())),
    ))
}

/// What a prompt shows: its texts, the data's URI and the data.
type Built = (Vec<LocalizedMessage>, String, Value);

impl<D: McpData> Shared<D> {
    /// The newest messages of a thread the grant shows, with their text
    /// where the client may read it.
    async fn thread_with_texts(&self, access: &Access, id: &ThreadId) -> Result<Value, Failure> {
        let mut value = self.thread_json(access, id).await?;
        let messages = value["messages"].as_array().cloned().unwrap_or_default();
        let skip = messages
            .len()
            .saturating_sub(PROMPT_MAX_THREAD_MESSAGES as usize);
        let mut out = Vec::new();
        for mut message in messages.into_iter().skip(skip) {
            let Some(mid) = message["id"].as_str().map(MessageId::new) else {
                continue;
            };
            if self.readable_message(access, &mid).await.is_ok()
                && let Ok(body) = self.data.message_body(&mid).await
            {
                message["text"] = json!(body.text);
            }
            out.push(message);
        }
        value["messages"] = Value::Array(out);
        Ok(value)
    }
}

async fn summarize_thread<D: McpData>(
    shared: &Shared<D>,
    access: &Access,
    a: SummarizeThreadArgs,
) -> Result<Built, Failure> {
    access.require(McpScope::Read)?;
    let id = ThreadId::new(a.thread_id);
    let data = shared.thread_with_texts(access, &id).await?;
    Ok((
        vec![LocalizedMessage::new(
            McpPrompt::SummarizeThread.instructions_id(),
        )],
        crate::uri::ResourceRef::Thread(id).uri(),
        data,
    ))
}

async fn triage_folder<D: McpData>(
    shared: &Shared<D>,
    access: &Access,
    a: TriageFolderArgs,
) -> Result<Built, Failure> {
    access.require(McpScope::Read)?;
    let folder = FolderId::new(a.folder_id);
    shared.require_folder(access, &folder).await?;
    let limit = match a.limit.as_deref().map(str::trim) {
        Some(text) => text
            .parse::<u32>()
            .map_err(|_| Failure::Api(ApiError::invalid_request("limit is a number")))?
            .clamp(1, TRIAGE_MAX_LIMIT),
        None => TRIAGE_DEFAULT_LIMIT,
    };
    let mut unread: Vec<MessageSummary> = Vec::new();
    let mut offset = 0;
    while offset < TRIAGE_SCAN && unread.len() < limit as usize {
        let page = shared.data.messages(&folder, offset, MAX_PAGE).await?;
        let returned = u32::try_from(page.messages.len()).unwrap_or(u32::MAX);
        unread.extend(
            page.messages
                .into_iter()
                .filter(|m| !m.flags.seen && access.allows_message(m)),
        );
        offset = offset.saturating_add(returned);
        if returned == 0 || offset >= page.total {
            break;
        }
    }
    unread.truncate(limit as usize);
    Ok((
        vec![LocalizedMessage::new(
            McpPrompt::TriageFolder.instructions_id(),
        )],
        crate::uri::folder_messages(&folder),
        json!({
            "folder_id": folder,
            "unread": unread.iter().map(summary_json).collect::<Vec<_>>(),
        }),
    ))
}

async fn draft_reply<D: McpData>(
    shared: &Shared<D>,
    access: &Access,
    a: DraftReplyArgs,
) -> Result<Built, Failure> {
    access.require(McpScope::Read)?;
    let id = MessageId::new(a.message_id);
    let summary = shared.readable_message(access, &id).await?;
    let reply_all = a
        .reply_all
        .as_deref()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"));
    let thread = if let Some(thread) = &summary.thread_id {
        shared.thread_with_texts(access, thread).await?
    } else {
        let body = shared.data.message_body(&id).await?;
        let mut message = summary_json(&summary);
        message["text"] = json!(body.text);
        json!({ "messages": [message] })
    };
    let mut texts = vec![
        LocalizedMessage::new(McpPrompt::DraftReply.instructions_id()),
        LocalizedMessage::new("mcp-prompt-draft-reply-target")
            .arg("message_id", id.as_str())
            .arg("reply_all", if reply_all { "true" } else { "false" }),
    ];
    if let Some(wishes) = a.instructions.filter(|w| !w.trim().is_empty()) {
        texts.push(
            LocalizedMessage::new("mcp-prompt-draft-reply-wishes").arg("instructions", wishes),
        );
    }
    Ok((
        texts,
        crate::uri::message(&id),
        json!({
            "reply_to_message_id": id,
            "reply_all": reply_all,
            "thread": thread,
        }),
    ))
}

async fn prepare_meeting<D: McpData>(
    shared: &Shared<D>,
    access: &Access,
    a: PrepareMeetingArgs,
) -> Result<Built, Failure> {
    access.require(McpScope::Read)?;
    let calendars = shared.visible_calendars(access).await?;
    let missing = || {
        Failure::Api(ApiError::not_found(format!(
            "event {} not found",
            a.event_id
        )))
    };
    let event = match shared
        .data
        .query(Request::GetEvent {
            event_id: EventId::new(a.event_id.clone()),
        })
        .await
    {
        Ok(Response::PimEvent { event }) => *event,
        _ => return Err(missing()),
    };
    if !calendars.iter().any(|c| c.id == event.calendar_id) {
        return Err(missing());
    }
    let fields = match &a.occurrence {
        Some(occurrence) => event
            .overrides
            .iter()
            .find(|o| {
                crate::writes::pim::parse_time(occurrence, None)
                    .is_ok_and(|t| t.date_part() == o.recurrence_id.date_part())
            })
            .map_or_else(|| event.fields.clone(), |o| o.fields.clone()),
        None => event.fields.clone(),
    };
    let mut attendees = Vec::new();
    for attendee in fields.attendees.iter().take(MEETING_MAX_ATTENDEES) {
        let contact = shared
            .search_contacts_json(
                access,
                crate::pim::SearchContactsArgs {
                    query: attendee.email.clone(),
                    limit: Some(1),
                },
            )
            .await
            .ok()
            .and_then(|v| v["matches"].get(0).cloned());
        attendees.push(json!({
            "email": attendee.email,
            "name": attendee.name,
            "role": attendee.role,
            "status": attendee.status,
            "contact": contact,
        }));
    }
    let related = related_mail(shared, access, &fields.title).await?;
    Ok((
        vec![LocalizedMessage::new(
            McpPrompt::PrepareMeeting.instructions_id(),
        )],
        format!("{}/events/{}", crate::uri::CALENDARS, event.id),
        json!({
            "event": {
                "id": event.id,
                "calendar_id": event.calendar_id,
                "occurrence": a.occurrence,
                "fields": fields,
                "recurring": event.is_recurring(),
            },
            "attendees": attendees,
            "related_messages": related,
        }),
    ))
}

/// Recent mail whose text matches the meeting's title, as far as the
/// grant shows it.
async fn related_mail<D: McpData>(
    shared: &Shared<D>,
    access: &Access,
    title: &str,
) -> Result<Vec<Value>, Failure> {
    let phrase: String = title
        .chars()
        .filter(|c| !matches!(c, '"' | '(' | ')' | ':'))
        .collect();
    if phrase.trim().is_empty() {
        return Ok(Vec::new());
    }
    let hide_encrypted =
        !access.has(McpScope::Encrypted) && shared.data.index_holds_decrypted().await?;
    let query = SearchQuery {
        text: format!("\"{}\"", phrase.trim()),
        account_id: None,
        folder_id: None,
        offset: 0,
        limit: MAX_PAGE,
        sort: SearchSort::Date,
    };
    let mut out = Vec::new();
    if let SearchOutcome::Hits { hits, .. } = shared.data.search(query).await? {
        for hit in hits {
            if out.len() >= MEETING_MAX_RELATED as usize {
                break;
            }
            if !access.allows_message(&hit.message) {
                continue;
            }
            if hide_encrypted && shared.data.is_encrypted(&hit.message.id).await? {
                continue;
            }
            out.push(summary_json(&hit.message));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_become_strings() {
        let mut request = GetPromptRequestParams::new("triage_folder");
        request.arguments = Some(
            serde_json::from_value(json!({"folder_id": "f1", "limit": 10, "x": null})).unwrap(),
        );
        let value = string_arguments(&request);
        assert_eq!(value, json!({"folder_id": "f1", "limit": "10"}));
        let parsed: TriageFolderArgs = arguments(value).unwrap();
        assert_eq!(parsed.limit.as_deref(), Some("10"));
        assert!(arguments::<SummarizeThreadArgs>(json!({})).is_err());
    }

    #[test]
    fn every_argument_is_described() {
        for prompt in McpPrompt::ALL {
            for (name, _) in prompt.arguments() {
                assert!(!argument_description(prompt, name).is_empty(), "{name}");
            }
        }
    }
}
