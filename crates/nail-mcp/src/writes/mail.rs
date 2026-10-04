//! Mail writes: `update_messages`, `delete_messages`, `create_draft`,
//! `update_draft`, `send_message`, `edit_open_draft`.

use std::collections::HashMap;

use base64::Engine as _;
use nail_api::accounts::ComposerMode;
use nail_api::agents::{ActionKind, ActionTargetKind};
use nail_api::compose::Draft;
use nail_api::mcp::McpScope;
use nail_api::mcp::tools::{
    CreateDraftArgs, DeleteMessagesArgs, DraftAttachmentArg, EditOpenDraftArgs,
    MAX_AGENT_ATTACHMENT, MAX_BATCH, McpEditOp, McpTextFormat, SendMessageArgs, UpdateDraftArgs,
    UpdateMessagesArgs,
};
use nail_api::plugins::{ComposerEdit, ComposerEditOp, ComposerTextFormat};
use nail_api::{
    AccountId, ApiError, DraftContext, DraftContextMode, DraftId, FlagChange, Folder, FolderId,
    FolderRole, IdentityId, LocalizedMessage, MessageId, MessageSummary, MoveTarget, Request,
    Response,
};

use super::plan::{
    Action, Plan, Upload, UploadSource, accounts, addresses_text, change, count, invalid, joined,
    not_found, parse_addresses, target, unexpected,
};
use super::{Ctx, audit, audit_bools};
use crate::access::Denial;
use crate::data::McpData;
use crate::server::Failure;

/// Longest text `edit_open_draft` inserts (characters).
const MAX_EDIT_TEXT: usize = 256 * 1024;

impl<D: McpData> Ctx<'_, D> {
    /// The messages a write names: every one must exist and lie in a
    /// granted account and a folder that is not excluded.
    pub(crate) async fn target_messages(
        &self,
        ids: &[String],
    ) -> Result<Vec<MessageSummary>, Failure> {
        if ids.is_empty() {
            return Err(invalid("message_ids is empty"));
        }
        if ids.len() > MAX_BATCH {
            return Err(invalid(format!("at most {MAX_BATCH} messages per call")));
        }
        let mut out: Vec<MessageSummary> = Vec::new();
        for id in ids {
            if out.iter().any(|m| m.id.as_str() == id) {
                continue;
            }
            let summary = self
                .shared
                .data
                .message_summary(&MessageId::new(id.clone()))
                .await?;
            self.access.require_place(&summary)?;
            out.push(summary);
        }
        Ok(out)
    }

    /// One message by id (place checked, no `read` needed).
    pub(crate) async fn target_message(&self, id: &str) -> Result<MessageSummary, Failure> {
        let summary = self
            .shared
            .data
            .message_summary(&MessageId::new(id))
            .await?;
        self.access.require_place(&summary)?;
        Ok(summary)
    }

    /// The folders of the accounts of `messages`.
    async fn folders_of(
        &self,
        accounts: &[AccountId],
    ) -> Result<HashMap<FolderId, Folder>, Failure> {
        let mut out = HashMap::new();
        for account in accounts {
            for folder in self.shared.data.folders(account).await? {
                out.insert(folder.id.clone(), folder);
            }
        }
        Ok(out)
    }

    /// A folder a write moves to: in a granted account, not excluded.
    async fn writable_folder(&self, id: &FolderId) -> Result<Folder, Failure> {
        if !self.access.allows_folder(id) {
            return Err(Denial::Folder(id.clone()).into());
        }
        for account in self.shared.data.accounts().await? {
            if !self.access.allows_account(&account.id) {
                continue;
            }
            if let Ok(folders) = self.shared.data.folders(&account.id).await
                && let Some(folder) = folders.into_iter().find(|f| &f.id == id)
            {
                return Ok(folder);
            }
        }
        Err(Denial::Folder(id.clone()).into())
    }

    /// The account of an identity, if the grant covers it.
    async fn identity_account(&self, identity: &IdentityId) -> Result<AccountId, Failure> {
        let account = self
            .shared
            .data
            .identities()
            .await?
            .into_iter()
            .find(|i| &i.id == identity)
            .map(|i| i.account_id)
            .ok_or_else(|| not_found(format!("identity {identity}")))?;
        if self.access.allows_account(&account) {
            Ok(account)
        } else {
            Err(Denial::Account(account).into())
        }
    }

    /// A saved draft the client may change: its identity's account is
    /// granted, and the message it replies to or forwards (whose text it
    /// may quote) is readable.
    async fn writable_draft(&self, id: &DraftId) -> Result<(Draft, AccountId), Failure> {
        let draft = self.shared.data.draft(id).await?;
        let account = self.identity_account(&draft.identity_id).await?;
        let original = draft
            .context
            .as_ref()
            .map(|c| c.message_id.clone())
            .or_else(|| match &draft.automatic {
                Some(nail_api::AutomaticMail::Redirect { message_id }) => Some(message_id.clone()),
                _ => None,
            });
        if let Some(original) = original {
            self.shared.readable_message(self.access, &original).await?;
        }
        Ok((draft, account))
    }

    /// Checks attachment arguments and turns them into uploads.
    async fn uploads(&self, list: &[DraftAttachmentArg]) -> Result<Vec<Upload>, Failure> {
        let mut out = Vec::new();
        for a in list {
            let filename = a.filename.trim();
            if filename.is_empty() || filename.contains(['/', '\\']) {
                return Err(invalid(format!(
                    "invalid attachment file name {:?}",
                    a.filename
                )));
            }
            let source = match (&a.data_base64, &a.from_message_id, a.from_attachment_index) {
                (Some(data), None, None) => {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(data.trim())
                        .map_err(|_| invalid(format!("{filename}: data_base64 is not base64")))?;
                    if bytes.len() > MAX_AGENT_ATTACHMENT {
                        return Err(Failure::Api(ApiError::new(
                            nail_api::ErrorCode::TooLarge,
                            format!("{filename} is larger than {MAX_AGENT_ATTACHMENT} bytes"),
                        )));
                    }
                    UploadSource::Data {
                        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                    }
                }
                (None, Some(message), Some(index)) => {
                    let message = MessageId::new(message.clone());
                    self.shared.readable_message(self.access, &message).await?;
                    self.access.require(McpScope::Attachments)?;
                    let body = self.shared.data.message_body(&message).await?;
                    if crate::data::attachment_info(&body, index).is_none() {
                        return Err(not_found(format!("attachment {index} of {message}")));
                    }
                    UploadSource::Message {
                        message_id: message,
                        index,
                    }
                }
                _ => {
                    return Err(invalid(
                        "an attachment takes data_base64, or from_message_id with from_attachment_index",
                    ));
                }
            };
            out.push(Upload {
                filename: filename.to_owned(),
                mime: a
                    .mime_type
                    .clone()
                    .filter(|m| m.contains('/'))
                    .unwrap_or_else(|| mime_of(filename).to_owned()),
                source,
            });
        }
        Ok(out)
    }
}

/// A MIME type from a file name (the common ones; else octet-stream).
fn mime_of(filename: &str) -> &'static str {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "txt" => "text/plain",
        "csv" => "text/csv",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "ics" => "text/calendar",
        "vcf" => "text/vcard",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

fn mode_of(format: McpTextFormat) -> ComposerMode {
    match format {
        McpTextFormat::Text => ComposerMode::Plain,
        McpTextFormat::Markdown => ComposerMode::Markdown,
        McpTextFormat::Html => ComposerMode::Html,
    }
}

/// The draft's body source in its mode.
fn body_of(draft: &Draft) -> &str {
    match draft.mode {
        ComposerMode::Plain => &draft.text,
        ComposerMode::Html => draft.html.as_deref().unwrap_or_default(),
        ComposerMode::Markdown => draft.markdown.as_deref().unwrap_or_default(),
    }
}

/// Sets the body source (and the mode).
fn set_body(draft: &mut Draft, mode: ComposerMode, body: String) {
    draft.mode = mode;
    match mode {
        ComposerMode::Plain => {
            draft.text = body;
            draft.html = None;
            draft.markdown = None;
        }
        ComposerMode::Html => {
            draft.html = Some(body);
            draft.markdown = None;
        }
        ComposerMode::Markdown => {
            draft.markdown = Some(body);
            draft.html = None;
        }
    }
}

fn subject_or_none(subject: &str) -> Option<String> {
    (!subject.is_empty()).then(|| subject.to_owned())
}

fn attachment_names(draft: &Draft) -> Option<String> {
    joined(draft.attachments.iter().map(|a| a.filename.as_str()))
}

/// Whether moving into a folder of this role deletes (Trash; Junk, which
/// servers empty on their own).
pub(crate) fn deletes(role: FolderRole) -> bool {
    matches!(role, FolderRole::Trash | FolderRole::Junk)
}

/// `update_messages`.
pub(crate) async fn update_messages<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: UpdateMessagesArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Organize)?;
    let tags = |list: &[String]| -> Result<Vec<String>, Failure> {
        list.iter()
            .map(|t| {
                let t = t.trim();
                if t.is_empty() || t.chars().any(char::is_whitespace) {
                    Err(invalid(format!("invalid tag {t:?}")))
                } else {
                    Ok(t.to_owned())
                }
            })
            .collect()
    };
    let (add_tags, remove_tags) = (tags(&a.add_tags)?, tags(&a.remove_tags)?);
    let moves = usize::from(a.move_to_folder_id.is_some())
        + usize::from(a.archive)
        + usize::from(a.snooze_until.is_some());
    if moves > 1 {
        return Err(invalid(
            "choose one of move_to_folder_id, archive and snooze_until",
        ));
    }
    let flags = FlagChange {
        seen: a.read,
        flagged: a.flagged,
        answered: None,
        draft: None,
        add_tags: add_tags.clone(),
        remove_tags: remove_tags.clone(),
    };
    if flags.is_empty() && moves == 0 {
        return Err(invalid("nothing to change"));
    }
    let messages = ctx.target_messages(&a.message_ids).await?;
    let ids: Vec<MessageId> = messages.iter().map(|m| m.id.clone()).collect();
    let account_ids = accounts(messages.iter().map(|m| &m.account_id));
    let folders = ctx.folders_of(&account_ids).await?;
    let folder_name = |id: &FolderId| {
        folders
            .get(id)
            .map_or_else(|| id.as_str().to_owned(), |f| f.name.clone())
    };
    let mut changes = Vec::new();
    let mut audit_lines = Vec::new();
    let mut requests = Vec::new();
    if let Some(read) = a.read {
        changes.push(change(
            "action-field-read",
            ctx.yes_no_mixed(messages.iter().map(|m| m.flags.seen)),
            Some(ctx.yes_no(read)),
        ));
        audit_lines.push(audit(
            "read",
            audit_bools(messages.iter().map(|m| m.flags.seen)),
            Some(read.to_string()),
        ));
    }
    if let Some(flagged) = a.flagged {
        changes.push(change(
            "action-field-flagged",
            ctx.yes_no_mixed(messages.iter().map(|m| m.flags.flagged)),
            Some(ctx.yes_no(flagged)),
        ));
        audit_lines.push(audit(
            "flagged",
            audit_bools(messages.iter().map(|m| m.flags.flagged)),
            Some(flagged.to_string()),
        ));
    }
    if !add_tags.is_empty() || !remove_tags.is_empty() {
        let before = joined(
            messages
                .iter()
                .flat_map(|m| m.tags.iter().map(String::as_str)),
        );
        let after = joined(
            messages
                .iter()
                .flat_map(|m| m.tags.iter().map(String::as_str))
                .chain(add_tags.iter().map(String::as_str))
                .filter(|t| !remove_tags.iter().any(|r| r == t)),
        );
        changes.push(change("action-field-tags", before.clone(), after.clone()));
        audit_lines.push(audit("tags", before, after));
    }
    if !flags.is_empty() {
        requests.push(Request::UpdateFlags {
            message_ids: ids.clone(),
            change: flags,
        });
    }
    let before_folders = joined(messages.iter().map(|m| m.folder_id.as_str()));
    let before_names = {
        let names: Vec<String> = messages.iter().map(|m| folder_name(&m.folder_id)).collect();
        joined(names.iter().map(String::as_str))
    };
    // A move into Trash (or Junk, which servers empty) deletes: it asks
    // like `delete_messages` (review M4a).
    let mut kind = ActionKind::Organize;
    if let Some(folder) = &a.move_to_folder_id {
        let folder = ctx.writable_folder(&FolderId::new(folder.clone())).await?;
        if deletes(folder.role) {
            kind = ActionKind::Trash;
        }
        if let Some(other) = messages.iter().find(|m| m.account_id != folder.account_id) {
            return Err(invalid(format!(
                "message {} is not in the folder's account {}",
                other.id, folder.account_id
            )));
        }
        changes.push(change(
            "action-field-folder",
            before_names.clone(),
            Some(folder.name.clone()),
        ));
        audit_lines.push(audit(
            "folder",
            before_folders.clone(),
            Some(folder.id.as_str().to_owned()),
        ));
        requests.push(Request::MoveMessages {
            message_ids: ids.clone(),
            target: MoveTarget::FolderId(folder.id),
        });
    }
    if a.archive {
        let archive = folders
            .values()
            .filter(|f| f.role == FolderRole::Archive)
            .map(|f| f.name.as_str());
        changes.push(change("action-field-folder", before_names, joined(archive)));
        audit_lines.push(audit("folder", before_folders, Some("archive".into())));
        requests.push(Request::MoveMessages {
            message_ids: ids.clone(),
            target: MoveTarget::Role(FolderRole::Archive),
        });
    }
    if let Some(until) = &a.snooze_until {
        let at = chrono::DateTime::parse_from_rfc3339(until)
            .map_err(|_| invalid(format!("invalid time {until:?}")))?;
        if at.timestamp() <= crate::server::now() {
            return Err(invalid("snooze_until is in the past"));
        }
        changes.push(change(
            "action-field-snoozed-until",
            None,
            Some(until.clone()),
        ));
        audit_lines.push(audit("snoozed_until", None, Some(until.clone())));
        requests.push(Request::SnoozeMessages {
            message_ids: ids,
            until: until.clone(),
            wake_folder_id: None,
        });
    }
    let summary = if kind == ActionKind::Trash {
        "confirmation-summary-trash"
    } else {
        "confirmation-summary-organize"
    };
    Ok(Plan {
        kind,
        summary: LocalizedMessage::new(summary)
            .arg("actor", ctx.actor())
            .arg("count", messages.len()),
        targets: messages
            .iter()
            .map(|m| target(ActionTargetKind::Message, m.id.as_str(), &m.subject))
            .collect(),
        changes,
        accounts: account_ids,
        audit: audit_lines,
        sends: 0,
        action: Action::Requests { requests },
    })
}

/// `delete_messages`: Trash, or permanent with its own scope; always
/// confirmed.
pub(crate) async fn delete_messages<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: DeleteMessagesArgs,
) -> Result<Plan, Failure> {
    let kind = if a.permanent {
        ActionKind::PermanentDelete
    } else {
        ActionKind::Trash
    };
    for scope in kind.mcp_scopes() {
        ctx.access.require(*scope)?;
    }
    let messages = ctx.target_messages(&a.message_ids).await?;
    let account_ids = accounts(messages.iter().map(|m| &m.account_id));
    let folders = ctx.folders_of(&account_ids).await?;
    let names: Vec<String> = messages
        .iter()
        .map(|m| {
            folders
                .get(&m.folder_id)
                .map_or_else(|| m.folder_id.as_str().to_owned(), |f| f.name.clone())
        })
        .collect();
    let after = if a.permanent {
        None
    } else {
        joined(
            folders
                .values()
                .filter(|f| f.role == FolderRole::Trash)
                .map(|f| f.name.as_str()),
        )
    };
    let summary_id = if a.permanent {
        "confirmation-summary-permanent-delete"
    } else {
        "confirmation-summary-trash"
    };
    Ok(Plan {
        kind,
        summary: LocalizedMessage::new(summary_id)
            .arg("actor", ctx.actor())
            .arg("count", messages.len()),
        targets: messages
            .iter()
            .map(|m| target(ActionTargetKind::Message, m.id.as_str(), &m.subject))
            .collect(),
        changes: vec![change(
            "action-field-folder",
            joined(names.iter().map(String::as_str)),
            after,
        )],
        accounts: account_ids,
        audit: vec![audit(
            "folder",
            joined(messages.iter().map(|m| m.folder_id.as_str())),
            Some(if a.permanent { "deleted" } else { "trash" }.into()),
        )],
        sends: 0,
        action: Action::Requests {
            requests: vec![Request::DeleteMessages {
                message_ids: messages.iter().map(|m| m.id.clone()).collect(),
                permanent: a.permanent,
            }],
        },
    })
}

/// The draft lines a confirmation shows, before → after.
fn draft_changes<D: McpData>(
    ctx: &Ctx<'_, D>,
    before: Option<&Draft>,
    after: &Draft,
    uploads: &[Upload],
) -> (
    Vec<nail_api::agents::ActionChange>,
    Vec<super::plan::AuditChange>,
) {
    let mut changes = Vec::new();
    let mut lines = Vec::new();
    let pairs = [
        ("action-field-to", "to", before.map(|d| &d.to), &after.to),
        ("action-field-cc", "cc", before.map(|d| &d.cc), &after.cc),
        (
            "action-field-bcc",
            "bcc",
            before.map(|d| &d.bcc),
            &after.bcc,
        ),
    ];
    for (label, field, old, new) in pairs {
        if old != Some(new) && !(old.is_none() && new.is_empty()) {
            changes.push(change(
                label,
                old.and_then(|o| addresses_text(o)),
                addresses_text(new),
            ));
            lines.push(audit(
                field,
                old.and_then(|o| count(o.len())),
                count(new.len()),
            ));
        }
    }
    if before.map(|d| d.subject.as_str()) != Some(after.subject.as_str()) {
        changes.push(change(
            "action-field-subject",
            before.and_then(|d| subject_or_none(&d.subject)),
            subject_or_none(&after.subject),
        ));
        lines.push(audit("subject", None, Some("changed".into())));
    }
    let (old_body, new_body) = (before.map(body_of), body_of(after));
    if old_body != Some(new_body) {
        changes.push(change(
            "action-field-body",
            old_body.and_then(subject_or_none),
            subject_or_none(new_body),
        ));
        lines.push(audit(
            "body",
            old_body.map(|b| format!("{} chars", b.chars().count())),
            Some(format!("{} chars", new_body.chars().count())),
        ));
    }
    let old_names = before.and_then(attachment_names);
    let new_names = joined(
        after
            .attachments
            .iter()
            .map(|a| a.filename.as_str())
            .chain(uploads.iter().map(|u| u.filename.as_str())),
    );
    if old_names != new_names {
        changes.push(change("action-field-attachments", old_names, new_names));
        lines.push(audit(
            "attachments",
            before.and_then(|d| count(d.attachments.len())),
            count(after.attachments.len() + uploads.len()),
        ));
    }
    let _ = ctx;
    (changes, lines)
}

/// `create_draft`.
pub(crate) async fn create_draft<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: CreateDraftArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Draft)?;
    if a.reply_to_message_id.is_some() && a.forward_message_id.is_some() {
        return Err(invalid(
            "reply_to_message_id and forward_message_id exclude each other",
        ));
    }
    if a.reply_all && a.reply_to_message_id.is_none() {
        return Err(invalid("reply_all needs reply_to_message_id"));
    }
    let context = match (&a.reply_to_message_id, &a.forward_message_id) {
        (Some(id), None) => Some(DraftContext {
            mode: if a.reply_all {
                DraftContextMode::ReplyAll
            } else {
                DraftContextMode::Reply
            },
            message_id: MessageId::new(id.clone()),
        }),
        (None, Some(id)) => Some(DraftContext {
            mode: DraftContextMode::Forward,
            message_id: MessageId::new(id.clone()),
        }),
        _ => None,
    };
    // A reply or forward quotes the original: the client must be able to
    // read it.
    let original = match &context {
        Some(c) => Some(
            ctx.shared
                .readable_message(ctx.access, &c.message_id)
                .await?,
        ),
        None => None,
    };
    // A forward carries the original's attachments: their bytes need the
    // attachments grant, as reading them would.
    if a.forward_message_id.is_some() && original.as_ref().is_some_and(|m| m.has_attachments) {
        ctx.access.require(McpScope::Attachments)?;
    }
    let identity = match &a.identity_id {
        Some(id) => {
            let id = IdentityId::new(id.clone());
            ctx.identity_account(&id).await?;
            Some(id)
        }
        None if original.is_none() && !ctx.access.all_accounts() => ctx
            .shared
            .data
            .identities()
            .await?
            .into_iter()
            .find(|i| ctx.access.allows_account(&i.account_id))
            .map(|i| i.id),
        None => None,
    };
    let mode = mode_of(a.format);
    let response = ctx
        .shared
        .data
        .query(Request::NewDraft {
            identity_id: identity,
            account_id: original.as_ref().map(|m| m.account_id.clone()),
            mode: Some(mode),
            context,
        })
        .await?;
    let Response::Draft { draft } = response else {
        return Err(unexpected(&response));
    };
    let mut draft = *draft;
    let account = ctx.identity_account(&draft.identity_id).await?;
    for (list, given) in [
        (&mut draft.to, &a.to),
        (&mut draft.cc, &a.cc),
        (&mut draft.bcc, &a.bcc),
    ] {
        if !given.is_empty() {
            *list = parse_addresses(given)?;
        }
    }
    if let Some(subject) = &a.subject {
        draft.subject = subject.clone();
    }
    // The agent's text goes above what the core prepared (signature,
    // quoted original).
    let prepared = body_of(&draft).to_owned();
    let body = match (a.body.is_empty(), prepared.is_empty()) {
        (true, _) => prepared,
        (false, true) => a.body.clone(),
        (false, false) if mode == ComposerMode::Html => format!("{}\n{prepared}", a.body),
        (false, false) => format!("{}\n\n{prepared}", a.body),
    };
    set_body(&mut draft, mode, body);
    let uploads = ctx.uploads(&a.attachments).await?;
    let (changes, audit_lines) = draft_changes(ctx, None, &draft, &uploads);
    let mut targets = vec![target(ActionTargetKind::Draft, "new", &draft.subject)];
    targets.extend(
        draft
            .to
            .iter()
            .chain(&draft.cc)
            .chain(&draft.bcc)
            .map(|r| target(ActionTargetKind::Recipient, r.email.as_str(), &r.email)),
    );
    Ok(Plan {
        kind: ActionKind::Draft,
        summary: LocalizedMessage::new("confirmation-summary-draft")
            .arg("actor", ctx.actor())
            .arg("subject", draft.subject.as_str()),
        targets,
        changes,
        accounts: vec![account],
        audit: audit_lines,
        sends: 0,
        action: Action::SaveDraft {
            draft: Box::new(draft),
            uploads,
        },
    })
}

/// `update_draft`.
pub(crate) async fn update_draft<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: UpdateDraftArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Draft)?;
    let id = DraftId::new(a.draft_id.clone());
    let (before, account) = ctx.writable_draft(&id).await?;
    let mut draft = before.clone();
    draft.id = Some(id.clone());
    if let Some(to) = &a.to {
        draft.to = parse_addresses(to)?;
    }
    if let Some(cc) = &a.cc {
        draft.cc = parse_addresses(cc)?;
    }
    if let Some(bcc) = &a.bcc {
        draft.bcc = parse_addresses(bcc)?;
    }
    if let Some(subject) = &a.subject {
        draft.subject = subject.clone();
    }
    match (&a.body, a.format) {
        (Some(body), format) => {
            let mode = format.map_or(draft.mode, mode_of);
            set_body(&mut draft, mode, body.clone());
        }
        (None, Some(format)) if mode_of(format) != draft.mode => {
            return Err(invalid("changing the format needs the new body"));
        }
        (None, _) => {}
    }
    let mut remove = a.remove_attachments.clone();
    remove.sort_unstable();
    remove.dedup();
    for index in remove.iter().rev() {
        let index = *index as usize;
        if index >= draft.attachments.len() {
            return Err(not_found(format!("attachment {index} of the draft")));
        }
        draft.attachments.remove(index);
    }
    let uploads = ctx.uploads(&a.add_attachments).await?;
    let (changes, audit_lines) = draft_changes(ctx, Some(&before), &draft, &uploads);
    if changes.is_empty() {
        return Err(invalid("nothing to change"));
    }
    Ok(Plan {
        kind: ActionKind::Draft,
        summary: LocalizedMessage::new("confirmation-summary-draft")
            .arg("actor", ctx.actor())
            .arg("subject", draft.subject.as_str()),
        targets: vec![target(ActionTargetKind::Draft, id.as_str(), &draft.subject)],
        changes,
        accounts: vec![account],
        audit: audit_lines,
        sends: 0,
        action: Action::SaveDraft {
            draft: Box::new(draft),
            uploads,
        },
    })
}

/// `send_message`.
pub(crate) async fn send_message<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: SendMessageArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Send)?;
    let id = DraftId::new(a.draft_id.clone());
    let (mut draft, account) = ctx.writable_draft(&id).await?;
    draft.id = Some(id.clone());
    if let Some(at) = &a.send_at {
        chrono::DateTime::parse_from_rfc3339(at)
            .map_err(|_| invalid(format!("invalid time {at:?}")))?;
    }
    let recipients: Vec<&nail_api::Address> =
        draft.to.iter().chain(&draft.cc).chain(&draft.bcc).collect();
    if recipients.is_empty() {
        return Err(invalid("the draft has no recipients"));
    }
    let mut targets = vec![target(ActionTargetKind::Draft, id.as_str(), &draft.subject)];
    targets.extend(recipients.iter().map(|r| {
        target(
            ActionTargetKind::Recipient,
            r.email.as_str(),
            &addresses_text(std::slice::from_ref(*r)).unwrap_or_default(),
        )
    }));
    let mut changes = vec![
        change("action-field-to", None, addresses_text(&draft.to)),
        change(
            "action-field-subject",
            None,
            subject_or_none(&draft.subject),
        ),
    ];
    if !draft.cc.is_empty() {
        changes.insert(
            1,
            change("action-field-cc", None, addresses_text(&draft.cc)),
        );
    }
    if !draft.bcc.is_empty() {
        changes.insert(
            1,
            change("action-field-bcc", None, addresses_text(&draft.bcc)),
        );
    }
    if let Some(names) = attachment_names(&draft) {
        changes.push(change("action-field-attachments", None, Some(names)));
    }
    if let Some(at) = &a.send_at {
        changes.push(change("action-field-send-at", None, Some(at.clone())));
    }
    Ok(Plan {
        kind: ActionKind::Send,
        summary: LocalizedMessage::new("confirmation-summary-send")
            .arg("actor", ctx.actor())
            .arg("subject", draft.subject.as_str())
            .arg("count", recipients.len()),
        targets,
        changes,
        accounts: vec![account],
        audit: vec![
            audit("recipients", None, count(recipients.len())),
            audit("send_at", None, a.send_at.clone()),
        ],
        sends: 1,
        // The draft as the confirmation shows it: sending refuses a draft
        // that changed since (review M4a; atomic in the core, ADR 0086).
        action: Action::SendDraft {
            draft_id: id,
            send_at: a.send_at,
            revision: nail_store::draft_revision(&draft),
        },
    })
}

/// `edit_open_draft`: an edit of the draft open in the composer.
pub(crate) async fn edit_open_draft<D: McpData>(
    ctx: &Ctx<'_, D>,
    a: EditOpenDraftArgs,
) -> Result<Plan, Failure> {
    ctx.access.require(McpScope::Context)?;
    ctx.access.require(McpScope::Draft)?;
    if a.text.chars().count() > MAX_EDIT_TEXT {
        return Err(invalid(format!("at most {MAX_EDIT_TEXT} characters")));
    }
    let context = ctx.shared.data.ui_context().await?;
    let Some(open) = context.draft_id else {
        return Err(not_found("an open draft"));
    };
    if a.draft_id.as_deref().is_some_and(|d| d != open.as_str()) {
        return Err(Failure::Api(ApiError::new(
            nail_api::ErrorCode::Conflict,
            "another draft is open in the composer",
        )));
    }
    let (draft, account) = ctx.writable_draft(&open).await?;
    let op = match a.op {
        McpEditOp::Insert => ComposerEditOp::Insert,
        McpEditOp::ReplaceSelection => ComposerEditOp::ReplaceSelection,
        McpEditOp::ReplaceAll => ComposerEditOp::ReplaceAll,
    };
    let format = match a.format {
        McpTextFormat::Text => ComposerTextFormat::Text,
        McpTextFormat::Markdown => ComposerTextFormat::Markdown,
        McpTextFormat::Html => ComposerTextFormat::Html,
    };
    let before = (a.op == McpEditOp::ReplaceAll)
        .then(|| subject_or_none(body_of(&draft)))
        .flatten();
    Ok(Plan {
        kind: ActionKind::Draft,
        summary: LocalizedMessage::new("confirmation-summary-draft")
            .arg("actor", ctx.actor())
            .arg("subject", draft.subject.as_str()),
        targets: vec![target(
            ActionTargetKind::Draft,
            open.as_str(),
            &draft.subject,
        )],
        changes: vec![change(
            "action-field-body",
            before,
            subject_or_none(&a.text),
        )],
        accounts: vec![account],
        audit: vec![audit(
            "body",
            None,
            Some(format!("{:?} {} chars", a.op, a.text.chars().count())),
        )],
        sends: 0,
        action: Action::ComposerEdit {
            draft_id: open,
            edit: ComposerEdit {
                op,
                text: a.text,
                format,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_follow_the_mode() {
        let mut draft = Draft::new(IdentityId::new("me@work.example"));
        set_body(&mut draft, ComposerMode::Markdown, "**hi**".into());
        assert_eq!(body_of(&draft), "**hi**");
        set_body(&mut draft, ComposerMode::Html, "<p>hi</p>".into());
        assert_eq!(body_of(&draft), "<p>hi</p>");
        assert_eq!(draft.markdown, None);
        set_body(&mut draft, ComposerMode::Plain, "hi".into());
        assert_eq!((body_of(&draft), draft.html.as_deref()), ("hi", None));
        assert_eq!(mode_of(McpTextFormat::Text), ComposerMode::Plain);
    }

    #[test]
    fn trash_and_junk_moves_delete() {
        assert!(deletes(FolderRole::Trash) && deletes(FolderRole::Junk));
        assert!(!deletes(FolderRole::Archive) && !deletes(FolderRole::Inbox));
    }

    #[test]
    fn mime_types_come_from_file_names() {
        assert_eq!(mime_of("plan.CSV"), "text/csv");
        assert_eq!(mime_of("photo.jpeg"), "image/jpeg");
        assert_eq!(mime_of("noext"), "application/octet-stream");
    }
}
