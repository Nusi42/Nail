//! Mail: the folder hierarchy (`mailFolders/delta`), message deltas per
//! folder (`mailFolders/{id}/messages/delta` with `@odata.deltaLink` as the
//! cursor; an expired link is [`DeltaOutcome::CursorExpired`]), raw MIME
//! (`$value`), replayed writes (`PATCH` of `isRead` / `flag` /
//! `categories`, `move`, `copy`, `permanentDelete`, MIME creates), Focused
//! Inbox (`inferenceClassification` and sender overrides), attachments and
//! sending (`sendMail` with base64 MIME).

use std::collections::BTreeSet;

use base64::Engine as _;
use nail_api::domain::Flags;
use nail_api::engine::{
    DeltaOutcome, Envelope, RemoteMessage, RemoteMessageChange, RemoteWrite, SendReceipt,
    SyncDelta, WriteOutcome,
};
use nail_api::exchange::engine::{
    ExchangeFolder, ExchangeFolderClass, ExchangeServerInfo, HierarchyDelta, HierarchyOutcome,
};
use nail_api::exchange::{ExchangeMessageInfo, ExchangeWellKnownFolder, InferenceClassification};
use nail_api::ops::FlagChange;
use reqwest::Method;
use serde_json::{Value, json};

use crate::batch::BatchRequest;
use crate::engine::{GraphEngine, WellKnown};
use crate::error::GraphError;
use crate::wire::{Attachment, MailFolder, MailboxSettings, Message, Page, User};
use crate::{mailbox_path, mime, path_segment, text};

/// The message properties a delta asks for.
pub const MESSAGE_SELECT: &str = "id,changeKey,conversationId,parentFolderId,isRead,isDraft,flag,categories,inferenceClassification,receivedDateTime,internetMessageId";

/// The folder properties a hierarchy delta asks for.
pub const FOLDER_SELECT: &str =
    "displayName,parentFolderId,childFolderCount,totalItemCount,unreadItemCount,isHidden";

/// The largest MIME message uploaded in one request (`sendMail` and
/// message creation take at most 4 MB per request; base64 adds a third).
pub const MAX_MIME_UPLOAD: usize = 4 * 1024 * 1024;

/// `PidTagLastVerbExecuted` (`0x1081`): 102 = replied, the closest Graph
/// has to IMAP's `\Answered`.
const LAST_VERB_EXECUTED: &str = "Integer 0x1081";

/// The well-known mail folders resolved per mailbox.
const MAIL_WELL_KNOWN: [ExchangeWellKnownFolder; 8] = [
    ExchangeWellKnownFolder::Root,
    ExchangeWellKnownFolder::Inbox,
    ExchangeWellKnownFolder::SentItems,
    ExchangeWellKnownFolder::Drafts,
    ExchangeWellKnownFolder::DeletedItems,
    ExchangeWellKnownFolder::JunkEmail,
    ExchangeWellKnownFolder::Archive,
    ExchangeWellKnownFolder::Outbox,
];

/// Whether a stored cursor is a Graph delta / paging link (as opposed to
/// garbage or a cursor of another engine).
fn link_cursor(engine: &GraphEngine, cursor: &str) -> bool {
    url::Url::parse(cursor).is_ok_and(|u| engine.client.same_origin(&u))
}

/// The flags of a message.
pub fn flags_of(message: &Message) -> Flags {
    Flags {
        seen: message.is_read.unwrap_or(false),
        flagged: message
            .flag
            .as_ref()
            .is_some_and(|f| f.flag_status == "flagged"),
        // Graph does not report `\Answered` in deltas; the core keeps the
        // local state (like Gmail).
        answered: false,
        draft: message.is_draft.unwrap_or(false),
    }
}

fn lower(mailbox: Option<&str>) -> Option<String> {
    mailbox.map(str::to_ascii_lowercase)
}

impl GraphEngine {
    /// The signed-in user, the mailbox's zone; also the account test.
    pub async fn server_info_graph(&self) -> Result<ExchangeServerInfo, GraphError> {
        let answers = self
            .client
            .batch(&[
                BatchRequest::get("/me?$select=id,displayName,mail,userPrincipalName"),
                BatchRequest::get("/me/mailboxSettings?$select=timeZone"),
            ])
            .await?;
        let user: User = answers[0].json("GET /me")?;
        // Reading the zone needs `MailboxSettings.Read`; without it the
        // zone is unknown.
        let settings: Option<MailboxSettings> = answers[1].json("GET /me/mailboxSettings").ok();
        Ok(ExchangeServerInfo {
            address: user
                .mail
                .or(user.user_principal_name)
                .unwrap_or_default()
                .to_ascii_lowercase(),
            display_name: user.display_name,
            server_version: None,
            time_zone: settings.and_then(|s| s.time_zone),
            // Exchange Online classifies every mailbox's mail.
            focused_inbox: true,
        })
    }

    /// The well-known mail folders of a mailbox (cached; one `$batch`).
    pub(crate) async fn well_known(&self, mailbox: Option<&str>) -> Result<WellKnown, GraphError> {
        let key = lower(mailbox);
        if let Some(known) = self.well_known.lock().await.get(&key) {
            return Ok(known.clone());
        }
        let base = mailbox_path(mailbox);
        let requests: Vec<BatchRequest> = MAIL_WELL_KNOWN
            .iter()
            .filter_map(|f| f.graph_name())
            .map(|name| BatchRequest::get(format!("{base}/mailFolders/{name}?$select=id")))
            .collect();
        let answers = self.client.batch(&requests).await?;
        let mut known = WellKnown::default();
        for (folder, answer) in MAIL_WELL_KNOWN.iter().zip(&answers) {
            match answer.json::<MailFolder>("GET mailFolders/{well-known}") {
                Ok(f) if !f.id.is_empty() => {
                    known.by_id.insert(f.id, *folder);
                }
                // A mailbox without an Archive folder, say.
                Ok(_) | Err(GraphError::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
        self.well_known.lock().await.insert(key, known.clone());
        Ok(known)
    }

    /// Forgets the cached well-known folders (after a resync).
    pub async fn forget_well_known(&self, mailbox: Option<&str>) {
        self.well_known.lock().await.remove(&lower(mailbox));
    }

    /// Folder changes of a mailbox since `cursor`.
    pub async fn hierarchy(
        &self,
        mailbox: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<HierarchyOutcome, GraphError> {
        if cursor.is_some_and(|c| !link_cursor(self, c)) {
            return Ok(HierarchyOutcome::CursorExpired);
        }
        let known = self.well_known(mailbox).await?;
        let root = known
            .id_of(ExchangeWellKnownFolder::Root)
            .map(str::to_owned);
        let mut delta = HierarchyDelta::default();
        let mut link = cursor.map(str::to_owned);
        let headers = [self.page_header()];
        for page_no in 0..self.options.pages_per_call.max(1) {
            let page: Result<Page<MailFolder>, GraphError> = match &link {
                Some(l) => {
                    self.client
                        .get_link_json(l, &headers, "mailFolders/delta")
                        .await
                }
                None => {
                    self.client
                        .get_json(
                            &format!("{}/mailFolders/delta", mailbox_path(mailbox)),
                            &[("$select", FOLDER_SELECT.to_owned())],
                            &headers,
                            "mailFolders/delta",
                        )
                        .await
                }
            };
            let page = match page {
                Ok(p) => p,
                Err(GraphError::SyncStateExpired(_)) if cursor.is_some() || page_no > 0 => {
                    return Ok(HierarchyOutcome::CursorExpired);
                }
                Err(e) => return Err(e),
            };
            for folder in page.value {
                if folder.removed.is_some() {
                    delta.removed.push(folder.id);
                    continue;
                }
                if folder.is_hidden == Some(true) || Some(&folder.id) == root.as_ref() {
                    continue;
                }
                let parent = folder
                    .parent_folder_id
                    .filter(|p| Some(p) != root.as_ref() && !p.is_empty());
                delta.upserted.push(ExchangeFolder {
                    well_known: known.by_id.get(&folder.id).copied(),
                    remote_id: folder.id,
                    change_key: None,
                    parent,
                    name: folder.display_name.unwrap_or_default(),
                    class: ExchangeFolderClass::Mail,
                    total: folder.total_item_count,
                    unread: folder.unread_item_count,
                });
            }
            match (page.next_link, page.delta_link) {
                (Some(next), _) => link = Some(next),
                (None, Some(delta_link)) => {
                    delta.cursor = delta_link;
                    delta.more = false;
                    return Ok(HierarchyOutcome::Delta(delta));
                }
                (None, None) => {
                    return Err(GraphError::Protocol(
                        "mailFolders/delta without a next or delta link".into(),
                    ));
                }
            }
        }
        delta.cursor = link.unwrap_or_default();
        delta.more = true;
        Ok(HierarchyOutcome::Delta(delta))
    }

    /// Message changes of one folder since `cursor` (see
    /// [`nail_api::exchange::engine::ExchangeMailEngine::folder_items`]).
    ///
    /// Graph does not tell new from changed messages: every message with
    /// its full metadata (a `receivedDateTime`) is in
    /// [`SyncDelta::added`] — the core upserts by remote id —, entries
    /// with only some properties are [`SyncDelta::changed`], and `@removed`
    /// ones (deleted or moved away) are [`SyncDelta::removed`].
    pub async fn messages_delta(
        &self,
        mailbox: Option<&str>,
        folder: &str,
        cursor: Option<&str>,
        since: Option<i64>,
    ) -> Result<DeltaOutcome, GraphError> {
        if cursor.is_some_and(|c| !link_cursor(self, c)) {
            return Ok(DeltaOutcome::CursorExpired);
        }
        let known = self.well_known(mailbox).await?;
        let inbox = folder.eq_ignore_ascii_case("inbox")
            || known.by_id.get(folder) == Some(&ExchangeWellKnownFolder::Inbox);
        let mut delta = SyncDelta::default();
        let mut link = cursor.map(str::to_owned);
        let headers = [self.page_header()];
        for page_no in 0..self.options.pages_per_call.max(1) {
            let page: Result<Page<Message>, GraphError> = match &link {
                Some(l) => {
                    self.client
                        .get_link_json(l, &headers, "messages/delta")
                        .await
                }
                None => {
                    let mut query = vec![("$select", MESSAGE_SELECT.to_owned())];
                    if let Some(utc) = since.and_then(text::unix_to_utc) {
                        query.push(("$filter", format!("receivedDateTime ge {utc}")));
                    }
                    self.client
                        .get_json(
                            &format!(
                                "{}/mailFolders/{}/messages/delta",
                                mailbox_path(mailbox),
                                path_segment(folder)
                            ),
                            &query,
                            &headers,
                            "messages/delta",
                        )
                        .await
                }
            };
            let page = match page {
                Ok(p) => p,
                Err(GraphError::SyncStateExpired(_)) if cursor.is_some() || page_no > 0 => {
                    return Ok(DeltaOutcome::CursorExpired);
                }
                Err(e) => return Err(e),
            };
            for message in page.value {
                self.fold_message(mailbox, folder, inbox, message, &mut delta);
            }
            match (page.next_link, page.delta_link) {
                (Some(next), _) => link = Some(next),
                (None, Some(delta_link)) => {
                    delta.cursor = delta_link;
                    return Ok(DeltaOutcome::Delta(delta));
                }
                (None, None) => {
                    return Err(GraphError::Protocol(
                        "messages/delta without a next or delta link".into(),
                    ));
                }
            }
        }
        delta.cursor = link.unwrap_or_default();
        delta.more = true;
        Ok(DeltaOutcome::Delta(delta))
    }

    fn fold_message(
        &self,
        mailbox: Option<&str>,
        folder: &str,
        inbox: bool,
        message: Message,
        delta: &mut SyncDelta,
    ) {
        if message.removed.is_some() {
            delta.removed.push(message.id);
            return;
        }
        let info = ExchangeMessageInfo {
            item_id: message.id.clone(),
            conversation_id: message.conversation_id.clone(),
            inference: if inbox {
                message
                    .inference_classification
                    .as_deref()
                    .and_then(InferenceClassification::parse)
            } else {
                None
            },
            mailbox: lower(mailbox),
        };
        let folder_id = message
            .parent_folder_id
            .clone()
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| folder.to_owned());
        if let Some(received) = &message.received_date_time {
            delta.added.push(RemoteMessage {
                remote_id: message.id.clone(),
                thread_id: message.conversation_id.clone(),
                folder: folder_id,
                flags: flags_of(&message),
                tags: message.categories.clone().unwrap_or_default(),
                internal_date: text::to_unix(received),
                size: None,
                raw: None,
                gmail: None,
                change_key: message.change_key.clone(),
                exchange: Some(info),
            });
        } else {
            let partial_flags =
                (message.is_read.is_some() || message.flag.is_some()).then(|| flags_of(&message));
            delta.changed.push(RemoteMessageChange {
                remote_id: message.id.clone(),
                folder: message.parent_folder_id.clone(),
                flags: partial_flags,
                tags: message.categories.clone(),
                gmail: None,
                change_key: message.change_key.clone(),
                exchange: Some(info),
            });
        }
    }

    /// The raw RFC 5322 bytes of a message (`$value`).
    pub async fn mime(
        &self,
        mailbox: Option<&str>,
        remote_id: &str,
    ) -> Result<Vec<u8>, GraphError> {
        self.client
            .get_bytes(
                &format!(
                    "{}/messages/{}/$value",
                    mailbox_path(mailbox),
                    path_segment(remote_id)
                ),
                "GET messages/$value",
            )
            .await
    }

    fn message_path(mailbox: Option<&str>, id: &str) -> String {
        format!("{}/messages/{}", mailbox_path(mailbox), path_segment(id))
    }

    /// Replays a queued write (idempotent where Graph allows).
    pub async fn replay(
        &self,
        mailbox: Option<&str>,
        write: &RemoteWrite,
    ) -> Result<WriteOutcome, GraphError> {
        let result = match write {
            RemoteWrite::SetFlags { remote_id, change } => self
                .set_flags(mailbox, remote_id, change)
                .await
                .map(|()| WriteOutcome::Done { remote_id: None }),
            RemoteWrite::Move {
                remote_id,
                to_folder,
                ..
            } => self
                .move_message(mailbox, remote_id, to_folder)
                .await
                .map(|id| WriteOutcome::Done {
                    remote_id: Some(id),
                }),
            RemoteWrite::Trash { remote_id } => self
                .move_message(mailbox, remote_id, "deleteditems")
                .await
                .map(|id| WriteOutcome::Done {
                    remote_id: Some(id),
                }),
            RemoteWrite::Delete { remote_id } => self
                .client
                .send_json(
                    Method::POST,
                    &format!("{}/permanentDelete", Self::message_path(mailbox, remote_id)),
                    None,
                    &[],
                    // Deleting twice ends in a 404 = gone.
                    true,
                    "POST messages/permanentDelete",
                )
                .await
                .map(|_| WriteOutcome::Done { remote_id: None }),
            RemoteWrite::Append { folder, raw, flags } => self
                .append(mailbox, folder, raw, *flags)
                .await
                .map(|id| WriteOutcome::Done {
                    remote_id: Some(id),
                }),
        };
        match result {
            Err(GraphError::NotFound(_)) => Ok(WriteOutcome::Gone),
            other => other,
        }
    }

    /// Sets read / flag / answered and adds or removes categories (tags).
    pub async fn set_flags(
        &self,
        mailbox: Option<&str>,
        remote_id: &str,
        change: &FlagChange,
    ) -> Result<(), GraphError> {
        let path = Self::message_path(mailbox, remote_id);
        let mut body = serde_json::Map::new();
        if let Some(seen) = change.seen {
            body.insert("isRead".into(), json!(seen));
        }
        if let Some(flagged) = change.flagged {
            let status = if flagged { "flagged" } else { "notFlagged" };
            body.insert("flag".into(), json!({ "flagStatus": status }));
        }
        if change.answered == Some(true) {
            body.insert(
                "singleValueExtendedProperties".into(),
                json!([{ "id": LAST_VERB_EXECUTED, "value": "102" }]),
            );
        }
        if !change.add_tags.is_empty() || !change.remove_tags.is_empty() {
            let current: Message = self
                .client
                .get_json(
                    &path,
                    &[("$select", "categories".to_owned())],
                    &[],
                    "GET message categories",
                )
                .await?;
            let current = current.categories.unwrap_or_default();
            let next = merge_categories(&current, &change.add_tags, &change.remove_tags);
            if next != current {
                body.insert("categories".into(), json!(next));
            }
        }
        if body.is_empty() {
            return Ok(());
        }
        self.client
            .send_json(
                Method::PATCH,
                &path,
                Some(&Value::Object(body)),
                &[],
                true,
                "PATCH message",
            )
            .await
            .map(drop)
    }

    /// Moves a message; answers its new id (Exchange ids change on moves).
    pub async fn move_message(
        &self,
        mailbox: Option<&str>,
        remote_id: &str,
        to_folder: &str,
    ) -> Result<String, GraphError> {
        self.move_or_copy(mailbox, remote_id, to_folder, "move")
            .await
    }

    /// Copies a message; answers the copy's id.
    pub async fn copy_message(
        &self,
        mailbox: Option<&str>,
        remote_id: &str,
        to_folder: &str,
    ) -> Result<String, GraphError> {
        self.move_or_copy(mailbox, remote_id, to_folder, "copy")
            .await
    }

    async fn move_or_copy(
        &self,
        mailbox: Option<&str>,
        remote_id: &str,
        to_folder: &str,
        action: &str,
    ) -> Result<String, GraphError> {
        let answer = self
            .client
            .send_json(
                Method::POST,
                &format!("{}/{action}", Self::message_path(mailbox, remote_id)),
                Some(&json!({ "destinationId": to_folder })),
                &[],
                false,
                &format!("POST messages/{action}"),
            )
            .await?;
        let message: Message = serde_json::from_value(answer)
            .map_err(|e| GraphError::Protocol(format!("{action}: {e}")))?;
        if message.id.is_empty() {
            return Err(GraphError::Protocol(format!("{action} answered no id")));
        }
        Ok(message.id)
    }

    /// Stores a raw message in a folder (a replayed append finds the copy
    /// it made before by its `Message-ID`); answers its id. Graph creates
    /// MIME uploads as drafts: outside Drafts, Outlook shows them as
    /// unsent (Graph cannot clear that for MIME creates). Messages over
    /// 4 MB go up in parts ([`GraphEngine::append_large`]).
    pub async fn append(
        &self,
        mailbox: Option<&str>,
        folder: &str,
        raw: &[u8],
        flags: Flags,
    ) -> Result<String, GraphError> {
        let base = mailbox_path(mailbox);
        if let Some(message_id) = mime::message_id(raw) {
            let found: Page<Message> = self
                .client
                .get_json(
                    &format!("{base}/mailFolders/{}/messages", path_segment(folder)),
                    &[
                        ("$filter", format!("internetMessageId eq '{message_id}'")),
                        ("$select", "id".to_owned()),
                        ("$top", "1".to_owned()),
                    ],
                    &[],
                    "find appended message",
                )
                .await?;
            if let Some(existing) = found.value.into_iter().next() {
                return Ok(existing.id);
            }
        }
        if crate::upload::needs_upload_session(raw) {
            return self.append_large(mailbox, folder, raw, flags).await;
        }
        let encoded = encode_mime(raw)?;
        let created = self
            .client
            .post_text(
                &format!("{base}/mailFolders/{}/messages", path_segment(folder)),
                encoded,
                "POST mailFolders/messages (MIME)",
            )
            .await?;
        let message: Message = serde_json::from_value(created)
            .map_err(|e| GraphError::Protocol(format!("append: {e}")))?;
        if message.id.is_empty() {
            return Err(GraphError::Protocol("append answered no id".into()));
        }
        let change = FlagChange {
            seen: Some(flags.seen),
            flagged: flags.flagged.then_some(true),
            answered: flags.answered.then_some(true),
            ..FlagChange::default()
        };
        self.set_flags(mailbox, &message.id, &change).await?;
        Ok(message.id)
    }

    /// Moves messages between Focused and Other; with `always`, adds a
    /// sender override for each distinct sender.
    pub async fn classify_messages(
        &self,
        mailbox: Option<&str>,
        remote_ids: &[String],
        classification: InferenceClassification,
        always: bool,
    ) -> Result<(), GraphError> {
        if remote_ids.is_empty() {
            return Ok(());
        }
        let base = mailbox_path(mailbox);
        let requests: Vec<BatchRequest> = remote_ids
            .iter()
            .map(|id| {
                BatchRequest::with_body(
                    Method::PATCH,
                    format!("{base}/messages/{}", path_segment(id)),
                    json!({ "inferenceClassification": classification.as_str() }),
                )
            })
            .collect();
        let answers = self.client.batch(&requests).await?;
        for answer in &answers {
            match answer.error("PATCH inferenceClassification") {
                None | Some(GraphError::NotFound(_)) => {}
                Some(e) => return Err(e),
            }
        }
        if !always {
            return Ok(());
        }
        let lookups: Vec<BatchRequest> = remote_ids
            .iter()
            .map(|id| {
                BatchRequest::get(format!("{base}/messages/{}?$select=from", path_segment(id)))
            })
            .collect();
        let mut senders: BTreeSet<(String, Option<String>)> = BTreeSet::new();
        for answer in self.client.batch(&lookups).await? {
            if let Ok(message) = answer.json::<Message>("GET message from")
                && let Some(from) = message.from
                && !from.email_address.address.is_empty()
            {
                senders.insert((
                    from.email_address.address.to_ascii_lowercase(),
                    from.email_address.name,
                ));
            }
        }
        for (address, name) in senders {
            let mut sender = json!({ "address": address });
            if let Some(name) = name {
                sender["name"] = json!(name);
            }
            self.client
                .send_json(
                    Method::POST,
                    &format!("{base}/inferenceClassification/overrides"),
                    Some(&json!({
                        "classifyAs": classification.as_str(),
                        "senderEmailAddress": sender,
                    })),
                    &[],
                    // Graph updates an existing override of the sender.
                    true,
                    "POST inferenceClassification/overrides",
                )
                .await?;
        }
        Ok(())
    }

    /// The attachments of a message (metadata).
    pub async fn attachments(
        &self,
        mailbox: Option<&str>,
        message_id: &str,
    ) -> Result<Vec<Attachment>, GraphError> {
        let page: Page<Attachment> = self
            .client
            .get_json(
                &format!("{}/attachments", Self::message_path(mailbox, message_id)),
                &[("$select", "id,name,contentType,size,isInline".to_owned())],
                &[],
                "GET messages/attachments",
            )
            .await?;
        Ok(page.value)
    }

    /// The raw content of a file attachment (`$value`).
    pub async fn attachment_content(
        &self,
        mailbox: Option<&str>,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, GraphError> {
        self.client
            .get_bytes(
                &format!(
                    "{}/attachments/{}/$value",
                    Self::message_path(mailbox, message_id),
                    path_segment(attachment_id)
                ),
                "GET attachments/$value",
            )
            .await
    }

    /// Creates a mail folder below `parent` (`None` = top level); answers
    /// its id.
    pub async fn create_folder(
        &self,
        mailbox: Option<&str>,
        parent: Option<&str>,
        name: &str,
    ) -> Result<String, GraphError> {
        let base = mailbox_path(mailbox);
        let path = match parent {
            Some(p) => format!("{base}/mailFolders/{}/childFolders", path_segment(p)),
            None => format!("{base}/mailFolders"),
        };
        let answer = self
            .client
            .send_json(
                Method::POST,
                &path,
                Some(&json!({ "displayName": name })),
                &[],
                false,
                "POST mailFolders",
            )
            .await?;
        let folder: MailFolder = serde_json::from_value(answer)
            .map_err(|e| GraphError::Protocol(format!("create folder: {e}")))?;
        Ok(folder.id)
    }

    /// Renames a mail folder.
    pub async fn rename_folder(
        &self,
        mailbox: Option<&str>,
        id: &str,
        name: &str,
    ) -> Result<(), GraphError> {
        self.client
            .send_json(
                Method::PATCH,
                &format!("{}/mailFolders/{}", mailbox_path(mailbox), path_segment(id)),
                Some(&json!({ "displayName": name })),
                &[],
                true,
                "PATCH mailFolders",
            )
            .await
            .map(drop)
    }

    /// Deletes a mail folder (a missing one is not an error).
    pub async fn delete_folder(&self, mailbox: Option<&str>, id: &str) -> Result<(), GraphError> {
        match self
            .client
            .send_json(
                Method::DELETE,
                &format!("{}/mailFolders/{}", mailbox_path(mailbox), path_segment(id)),
                None,
                &[],
                true,
                "DELETE mailFolders",
            )
            .await
        {
            Ok(_) | Err(GraphError::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Sends a raw message (at most 4 MB of MIME; larger ones go through
    /// [`GraphEngine::send_large`], and [`GraphEngine::send_message`]
    /// picks the mailbox and the way) through `sendMail` of the own
    /// mailbox (`None`) or another one (Send As / on behalf is the
    /// server's permission).
    /// Envelope recipients missing from the headers go into a `Bcc` header
    /// (Graph takes the recipients from the headers). Exchange files the
    /// message in Sent Items itself.
    pub async fn send_mime(
        &self,
        mailbox: Option<&str>,
        envelope: &Envelope,
        raw: &[u8],
    ) -> Result<SendReceipt, GraphError> {
        if envelope.to.is_empty() && mime::header_recipients(raw).is_empty() {
            return Err(GraphError::Rejected("no recipients".into()));
        }
        let raw = mime::with_envelope_bcc(raw, &envelope.to)?;
        let encoded = encode_mime(&raw)?;
        self.client
            .post_text(
                &format!("{}/sendMail", mailbox_path(mailbox)),
                encoded,
                "POST sendMail (MIME)",
            )
            .await?;
        Ok(SendReceipt {
            response: "202 accepted by Microsoft Graph".to_owned(),
        })
    }
}

/// A raw message as the base64 text Graph's MIME uploads take.
pub fn encode_mime(raw: &[u8]) -> Result<Vec<u8>, GraphError> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
    if encoded.len() > MAX_MIME_UPLOAD {
        return Err(GraphError::TooLarge(format!(
            "a MIME upload of {} bytes",
            raw.len()
        )));
    }
    Ok(encoded.into_bytes())
}

/// `current` categories with `add` added and `remove` removed (names
/// compare case-insensitively, like Outlook; the first spelling stays).
pub fn merge_categories(current: &[String], add: &[String], remove: &[String]) -> Vec<String> {
    let mut out: Vec<String> = current
        .iter()
        .filter(|c| !remove.iter().any(|r| r.eq_ignore_ascii_case(c)))
        .cloned()
        .collect();
    for tag in add {
        if !out.iter().any(|c| c.eq_ignore_ascii_case(tag)) && !tag.trim().is_empty() {
            out.push(tag.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_merge_case_insensitively() {
        let current = vec!["Red category".to_owned(), "Work".to_owned()];
        assert_eq!(
            merge_categories(
                &current,
                &["work".into(), "Travel".into(), " ".into()],
                &["red CATEGORY".into()]
            ),
            ["Work", "Travel"]
        );
        assert_eq!(merge_categories(&current, &[], &[]), current);
    }

    #[test]
    fn flags_from_messages() {
        let message: Message = serde_json::from_value(json!({
            "id": "m", "isRead": true, "isDraft": false, "flag": {"flagStatus": "flagged"}
        }))
        .unwrap();
        assert_eq!(
            flags_of(&message),
            Flags {
                seen: true,
                flagged: true,
                answered: false,
                draft: false
            }
        );
        let complete: Message =
            serde_json::from_value(json!({"id": "m", "flag": {"flagStatus": "complete"}})).unwrap();
        assert!(!flags_of(&complete).flagged);
    }

    #[test]
    fn mime_uploads_are_base64_and_bounded() {
        assert_eq!(
            encode_mime(b"Subject: x\r\n\r\n").unwrap(),
            b"U3ViamVjdDogeA0KDQo="
        );
        assert!(matches!(
            encode_mime(&vec![b'a'; MAX_MIME_UPLOAD]),
            Err(GraphError::TooLarge(_))
        ));
    }
}
