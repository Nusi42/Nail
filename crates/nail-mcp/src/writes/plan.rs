//! What a write tool will do: a [`Plan`] (the confirmation's view, the
//! audit's content-free before / after lines, the [`Action`] to run) and
//! running it ([`execute`]).
//!
//! Plans are built before anything changes, from reads only: the grant is
//! checked, the arguments validated and every item resolved. The action
//! is what runs once allowed or approved; it is also the confirmation's
//! sealed payload.

use std::collections::BTreeSet;

use nail_api::agents::{
    ActionAttribution, ActionChange, ActionKind, ActionTarget, ActionTargetKind, cut_text,
};
use nail_api::compose::Draft;
use nail_api::pim::{EditScope, EventFields, PimTime, Recurrence};
use nail_api::plugins::ComposerEdit;
use nail_api::{
    AccountId, Address, ApiError, CalendarId, DraftId, Event, LocalizedMessage, MessageId, Request,
    Response,
};
use serde::{Deserialize, Serialize};

use crate::data::McpData;
use crate::server::{Failure, Shared};

/// One before / after line of the audit log: field names and values that
/// carry no mailbox content (ids, flags, counts, times); the readable
/// lines with subjects and names live in the sealed confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AuditChange {
    /// The field (`flagged`, `folder`, `to`, …).
    pub field: &'static str,
    /// Before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// After.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

impl AuditChange {
    pub(crate) fn new(field: &'static str, before: Option<String>, after: Option<String>) -> Self {
        Self {
            field,
            before,
            after,
        }
    }
}

/// An attachment to upload into a draft when the action runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Upload {
    pub filename: String,
    pub mime: String,
    pub source: UploadSource,
}

/// Where an upload's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub(crate) enum UploadSource {
    /// The agent sent them (base64, already checked).
    Data { base64: String },
    /// An attachment of a message the client may read.
    Message { message_id: MessageId, index: u32 },
}

/// What runs once a write is allowed or approved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum Action {
    /// Client API requests, in order.
    Requests { requests: Vec<Request> },
    /// Upload the attachments, then save the draft.
    SaveDraft {
        draft: Box<Draft>,
        uploads: Vec<Upload>,
    },
    /// Create an event, then (optionally) mail the attendees.
    CreateEvent {
        calendar_id: CalendarId,
        fields: Box<EventFields>,
        recurrence: Option<Recurrence>,
        invite: bool,
    },
    /// Change an event, then (optionally) mail the attendees.
    UpdateEvent {
        event_id: nail_api::EventId,
        scope: EditScope,
        recurrence_id: Option<PimTime>,
        fields: Box<EventFields>,
        recurrence: Option<Recurrence>,
        invite: bool,
    },
    /// Edit the draft open in the composer (shown live).
    ComposerEdit {
        draft_id: DraftId,
        edit: ComposerEdit,
    },
}

/// A checked write, ready to ask about and run.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    /// The kind whose policy decides.
    pub kind: ActionKind,
    /// The confirmation's summary sentence.
    pub summary: LocalizedMessage,
    /// The items it touches.
    pub targets: Vec<ActionTarget>,
    /// Before / after for the user.
    pub changes: Vec<ActionChange>,
    /// The accounts concerned.
    pub accounts: Vec<AccountId>,
    /// Before / after for the audit log (no mailbox content).
    pub audit: Vec<AuditChange>,
    /// Messages it sends (against `sends_per_hour`).
    pub sends: u32,
    /// What runs.
    pub action: Action,
}

/// What running an action did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Executed {
    pub op_ids: Vec<String>,
    pub created: Vec<String>,
    /// A plug-in's before-send check stopped it (`<plug-in>: <reason>`).
    pub vetoed: Option<Vec<String>>,
}

/// An invalid argument (a tool error the agent can fix).
pub(crate) fn invalid(message: impl Into<String>) -> Failure {
    Failure::Api(ApiError::invalid_request(message))
}

/// Something named does not exist (or the grant hides it).
pub(crate) fn not_found(what: impl std::fmt::Display) -> Failure {
    Failure::Api(ApiError::not_found(format!("{what} not found")))
}

/// An unexpected answer of the core.
pub(crate) fn unexpected(response: &Response) -> Failure {
    Failure::Api(ApiError::internal(format!(
        "unexpected answer {}",
        response_type(response)
    )))
}

fn response_type(response: &Response) -> String {
    serde_json::to_value(response)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_owned))
        .unwrap_or_default()
}

/// A target line.
pub(crate) fn target(kind: ActionTargetKind, id: impl Into<String>, label: &str) -> ActionTarget {
    ActionTarget {
        kind,
        id: id.into(),
        label: cut_text(label),
    }
}

/// A before / after line with a contract field label (`action-field-*`).
pub(crate) fn change(field: &str, before: Option<String>, after: Option<String>) -> ActionChange {
    ActionChange {
        field: LocalizedMessage::new(field),
        before,
        after,
    }
}

/// Parses `Name <address>` or `address`.
pub(crate) fn parse_address(text: &str) -> Result<Address, Failure> {
    let text = text.trim();
    let (name, email) = match (text.rfind('<'), text.ends_with('>')) {
        (Some(open), true) => {
            let name = text[..open].trim().trim_matches('"').trim();
            (
                (!name.is_empty()).then(|| name.to_owned()),
                text[open + 1..text.len() - 1].trim(),
            )
        }
        _ => (None, text),
    };
    let plausible = email.len() <= 254
        && !email
            .chars()
            .any(|c| c.is_whitespace() || c == '<' || c == '>')
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
    if !plausible {
        return Err(invalid(format!("invalid address {text:?}")));
    }
    Ok(Address {
        name,
        email: email.to_owned(),
    })
}

/// Parses a list of addresses.
pub(crate) fn parse_addresses(list: &[String]) -> Result<Vec<Address>, Failure> {
    list.iter().map(|a| parse_address(a)).collect()
}

/// Addresses as one line (`Ada <ada@x>, bob@y`).
pub(crate) fn addresses_text(list: &[Address]) -> Option<String> {
    (!list.is_empty()).then(|| {
        list.iter()
            .map(|a| match &a.name {
                Some(name) => format!("{name} <{}>", a.email),
                None => a.email.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// A count for the audit log (`null` for none).
pub(crate) fn count(n: usize) -> Option<String> {
    (n > 0).then(|| n.to_string())
}

/// Distinct values in order of first appearance, joined.
pub(crate) fn joined<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut seen = BTreeSet::new();
    let list: Vec<&str> = values
        .into_iter()
        .filter(|v| !v.is_empty() && seen.insert(*v))
        .collect();
    (!list.is_empty()).then(|| list.join(", "))
}

/// A PIM time as text (`2026-10-01`, `2026-10-01T09:00:00 Europe/Berlin`).
pub(crate) fn pim_time_text(time: &PimTime) -> String {
    match time {
        PimTime::Date { date } => date.clone(),
        PimTime::Floating { local } => local.clone(),
        PimTime::Zoned { local, tz } => format!("{local} {tz}"),
    }
}

/// Distinct accounts.
pub(crate) fn accounts<'a>(list: impl IntoIterator<Item = &'a AccountId>) -> Vec<AccountId> {
    let mut out: Vec<AccountId> = Vec::new();
    for account in list {
        if !out.contains(account) {
            out.push(account.clone());
        }
    }
    out
}

fn op_ids(ids: &[nail_api::OpId]) -> impl Iterator<Item = String> + '_ {
    ids.iter().map(|id| id.as_str().to_owned())
}

impl<D: McpData> Shared<D> {
    async fn run(&self, by: &ActionAttribution, request: Request) -> Result<Response, ApiError> {
        self.data.write(by, request).await
    }

    /// Notes what one request's answer did.
    fn collect(response: &Response, creates: bool, done: &mut Executed) -> Result<(), Failure> {
        match response {
            Response::Queued { op_ids: ids } => done.op_ids.extend(op_ids(ids)),
            Response::Done | Response::Scheduled { .. } => {}
            Response::DraftSaved { draft_id } => {
                if creates {
                    done.created.push(draft_id.as_str().to_owned());
                }
            }
            Response::SendQueued { entry } => done.created.push(entry.id.as_str().to_owned()),
            Response::SendVetoed { vetoes, .. } => {
                done.vetoed = Some(
                    vetoes
                        .iter()
                        .map(|v| format!("{}: {}", v.plugin_name, v.reason))
                        .collect(),
                );
            }
            Response::EventSaved {
                event, op_ids: ids, ..
            } => {
                done.op_ids.extend(op_ids(ids));
                if creates {
                    done.created.push(event.id.as_str().to_owned());
                }
            }
            Response::ContactSaved {
                contact,
                op_ids: ids,
            } => {
                done.op_ids.extend(op_ids(ids));
                if creates {
                    done.created.push(contact.id.as_str().to_owned());
                }
            }
            Response::TaskSaved { task, op_ids: ids } => {
                done.op_ids.extend(op_ids(ids));
                if creates {
                    done.created.push(task.id.as_str().to_owned());
                }
            }
            Response::RuleSaved { rule } => done.created.push(rule.id.as_str().to_owned()),
            other => return Err(unexpected(other)),
        }
        Ok(())
    }

    async fn upload(&self, by: &ActionAttribution, upload: &Upload) -> Result<Response, Failure> {
        use base64::Engine as _;
        let data = match &upload.source {
            UploadSource::Data { base64 } => base64.clone(),
            UploadSource::Message { message_id, index } => {
                let attachment = self.data.attachment(message_id, *index).await?;
                if attachment.data.len() > nail_api::mcp::tools::MAX_AGENT_ATTACHMENT {
                    return Err(Failure::Api(ApiError::new(
                        nail_api::ErrorCode::TooLarge,
                        format!("attachment {index} of {message_id} is too large"),
                    )));
                }
                base64::engine::general_purpose::STANDARD.encode(&attachment.data)
            }
        };
        Ok(self
            .run(
                by,
                Request::UploadAttachment {
                    filename: upload.filename.clone(),
                    mime: upload.mime.clone(),
                    data,
                    content_id: None,
                },
            )
            .await?)
    }

    /// Runs an action for `by`.
    pub(crate) async fn execute(
        &self,
        by: &ActionAttribution,
        action: &Action,
    ) -> Result<Executed, Failure> {
        let mut done = Executed::default();
        match action {
            Action::Requests { requests } => {
                for request in requests {
                    let creates = matches!(
                        request,
                        Request::CreateContact { .. }
                            | Request::CreateTask { .. }
                            | Request::CreateEvent { .. }
                    );
                    let response = self.run(by, request.clone()).await?;
                    Self::collect(&response, creates, &mut done)?;
                    if done.vetoed.is_some() {
                        break;
                    }
                }
            }
            Action::SaveDraft { draft, uploads } => {
                let mut draft = draft.as_ref().clone();
                for upload in uploads {
                    match self.upload(by, upload).await? {
                        Response::AttachmentUploaded { attachment } => {
                            draft.attachments.push(attachment);
                        }
                        other => return Err(unexpected(&other)),
                    }
                }
                let creates = draft.id.is_none();
                let response = self
                    .run(
                        by,
                        Request::SaveDraft {
                            draft: draft.clone(),
                        },
                    )
                    .await?;
                Self::collect(&response, creates, &mut done)?;
            }
            Action::CreateEvent {
                calendar_id,
                fields,
                recurrence,
                invite,
            } => {
                let response = self
                    .run(
                        by,
                        Request::CreateEvent {
                            calendar_id: calendar_id.clone(),
                            fields: fields.clone(),
                            recurrence: recurrence.clone(),
                        },
                    )
                    .await?;
                let Response::EventSaved { event, .. } = &response else {
                    return Err(unexpected(&response));
                };
                let event_id = event.id.clone();
                Self::collect(&response, true, &mut done)?;
                if *invite {
                    let response = self
                        .run(
                            by,
                            Request::SendInvitations {
                                event_id,
                                recurrence_id: None,
                                attendees: Vec::new(),
                                comment: None,
                            },
                        )
                        .await?;
                    Self::collect(&response, false, &mut done)?;
                }
            }
            Action::UpdateEvent {
                event_id,
                scope,
                recurrence_id,
                fields,
                recurrence,
                invite,
            } => {
                let response = self
                    .run(
                        by,
                        Request::UpdateEvent {
                            event_id: event_id.clone(),
                            scope: *scope,
                            recurrence_id: recurrence_id.clone(),
                            fields: fields.clone(),
                            recurrence: recurrence.clone(),
                        },
                    )
                    .await?;
                Self::collect(&response, false, &mut done)?;
                if *invite {
                    let response = self
                        .run(
                            by,
                            Request::SendInvitations {
                                event_id: event_id.clone(),
                                recurrence_id: (*scope == EditScope::This)
                                    .then(|| recurrence_id.clone())
                                    .flatten(),
                                attendees: Vec::new(),
                                comment: None,
                            },
                        )
                        .await?;
                    Self::collect(&response, false, &mut done)?;
                }
            }
            Action::ComposerEdit { draft_id, edit } => {
                self.data.emit(Event::ComposerEditRequested {
                    draft_id: draft_id.clone(),
                    client_id: None,
                    by: Box::new(by.clone()),
                    edit: edit.clone(),
                });
                if let nail_api::agents::ActionActor::McpClient { client_id } = &by.actor {
                    self.data.emit(Event::McpContextAccess {
                        client_id: client_id.clone(),
                        client_name: by.name.clone(),
                        edit: true,
                    });
                }
            }
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_with_and_without_names() {
        let a = parse_address("Ada Lovelace <ada@example.org>").unwrap();
        assert_eq!(a.name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(a.email, "ada@example.org");
        let b = parse_address(" bob@example.org ").unwrap();
        assert_eq!((b.name, b.email.as_str()), (None, "bob@example.org"));
        let quoted = parse_address("\"Lovelace, Ada\" <ada@example.org>").unwrap();
        assert_eq!(quoted.name.as_deref(), Some("Lovelace, Ada"));
        for bad in [
            "",
            "ada",
            "a b@example.org",
            "@example.org",
            "ada@localhost",
        ] {
            assert!(parse_address(bad).is_err(), "{bad}");
        }
        assert_eq!(
            addresses_text(&[a, parse_address("c@example.org").unwrap()]).as_deref(),
            Some("Ada Lovelace <ada@example.org>, c@example.org")
        );
        assert_eq!(addresses_text(&[]), None);
    }

    #[test]
    fn small_helpers() {
        assert_eq!(joined(["a", "b", "a", ""]).as_deref(), Some("a, b"));
        assert_eq!(joined(Vec::<&str>::new()), None);
        assert_eq!(count(0), None);
        assert_eq!(count(3).as_deref(), Some("3"));
        assert_eq!(pim_time_text(&PimTime::date("2026-10-01")), "2026-10-01");
        assert_eq!(
            pim_time_text(&PimTime::zoned("2026-10-01T09:00:00", "Europe/Berlin")),
            "2026-10-01T09:00:00 Europe/Berlin"
        );
        let long = "x".repeat(600);
        assert!(
            target(ActionTargetKind::Message, "m1", &long)
                .label
                .ends_with('…')
        );
        let work: AccountId = "work".into();
        assert_eq!(accounts([&work, &work]).len(), 1);
    }

    #[test]
    fn actions_round_trip_as_payloads() {
        let action = Action::Requests {
            requests: vec![Request::DeleteMessages {
                message_ids: vec!["m1".into()],
                permanent: true,
            }],
        };
        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["action"], "requests");
        assert_eq!(serde_json::from_value::<Action>(value).unwrap(), action);
    }
}
