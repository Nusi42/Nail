//! [`GmailEngine`]: the Gmail API as a [`MailApiEngine`] and an
//! [`OutgoingTransport`] (ADR 0065).
//!
//! * **Full sync**: the profile's `historyId` is taken first (the cursor
//!   of the first page), then `messages.list` pages (Spam and Trash
//!   included, `after:` for "recent N days") with `format=minimal`
//!   metadata fetched through batch requests. Raw MIME is fetched per
//!   message ([`MailApiEngine::fetch_raw`], attachments included).
//! * **Incremental sync**: `history.list` from the stored `historyId`,
//!   folded into added / changed / removed messages; a `404` is
//!   [`DeltaOutcome::CursorExpired`] (full resync and merge). Long
//!   histories are read in slices ([`SyncDelta::more`]).
//! * **Writes** replay idempotently: flags and tags are
//!   `messages.modify` (tags = user labels, created on demand), moves are
//!   label changes / `trash` / `untrash`, appends look for the
//!   `Message-ID` first, a vanished message is [`WriteOutcome::Gone`].
//! * **Sending** is `messages.send` with the raw MIME; envelope
//!   recipients missing from the headers go into a `Bcc` header (Gmail
//!   takes recipients from the headers and removes `Bcc`), replies are
//!   filed into the original's thread.
//! * **Labels**: a cached label table maps user labels ↔ tags; the label
//!   requests of the core's label queue call [`GmailEngine::create_label`]
//!   and friends.

use std::collections::HashSet;

use nail_api::engine::{
    BoxFuture, DeltaOutcome, EngineError, Envelope, FullSyncPage, MailApiEngine, OutgoingTransport,
    RemoteFolder, RemoteMessage, RemoteWrite, SendReceipt, SyncDelta, WriteOutcome,
};
use nail_api::gmail::{GmailFolder, GmailLabelKind, GmailLabelSettings, LabelChange, system};
use nail_api::ops::FlagChange;
use tokio::sync::Mutex;

use crate::batch::DEFAULT_BATCH;
use crate::client::GmailClient;
use crate::error::GmailError;
use crate::history::{HISTORY_PAGE_SIZE, HistoryPage, fold_history};
use crate::labels::{GmailLabelInfo, LabelTable, label_info};
use crate::mapping::{is_chat, labels_for_insert, remote_change, remote_folders, remote_message};
use crate::messages::{Format, ListQuery, after_query};
use crate::wire::{History, Message};

/// Tunables of a [`GmailEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineOptions {
    /// Messages per full-sync page (`messages.list` `maxResults`).
    pub page_size: u32,
    /// Parts per batch request.
    pub batch_size: usize,
    /// Records per `history.list` page.
    pub history_page_size: u32,
    /// History pages read per [`MailApiEngine::changes`] call before it
    /// answers with [`SyncDelta::more`].
    pub history_pages_per_call: u32,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            page_size: 100,
            batch_size: DEFAULT_BATCH,
            history_page_size: HISTORY_PAGE_SIZE,
            history_pages_per_call: 10,
        }
    }
}

/// The Gmail engine of one account.
#[derive(Debug)]
pub struct GmailEngine {
    client: GmailClient,
    options: EngineOptions,
    labels: Mutex<Option<LabelTable>>,
}

fn gone_on_not_found(result: Result<(), GmailError>) -> Result<WriteOutcome, EngineError> {
    match result {
        Ok(()) => Ok(WriteOutcome::Done { remote_id: None }),
        Err(GmailError::NotFound(_)) => Ok(WriteOutcome::Gone),
        Err(e) => Err(e.into()),
    }
}

impl GmailEngine {
    /// An engine on `client` with default options.
    pub fn new(client: GmailClient) -> Self {
        Self {
            client,
            options: EngineOptions::default(),
            labels: Mutex::new(None),
        }
    }

    /// The engine of a configured `gmail` account: the API base from its
    /// `gmail` node, OAuth2 tokens from `tokens` with the account's
    /// refresh-token reference.
    pub fn for_account(
        account: &nail_api::AccountConfig,
        tokens: std::sync::Arc<dyn nail_api::engine::AccessTokenProvider>,
    ) -> Result<Self, GmailError> {
        use nail_api::accounts::{AuthConfig, IncomingServer};
        let IncomingServer::Gmail { server, .. } = &account.incoming else {
            return Err(GmailError::Config(account.id.to_string()));
        };
        let AuthConfig::OAuth2 { provider, .. } = &server.auth else {
            return Err(GmailError::Config(account.id.to_string()));
        };
        let secret = server
            .auth
            .secret_ref(&account.id)
            .ok_or_else(|| GmailError::Config(account.id.to_string()))?;
        let auth = crate::client::GmailAuth::OAuth2 {
            tokens,
            provider: provider.clone(),
            secret,
        };
        Ok(Self::new(GmailClient::new(
            &crate::api_base_for(server),
            auth,
        )?))
    }

    /// Replaces the options.
    #[must_use]
    pub fn with_options(mut self, options: EngineOptions) -> Self {
        self.options = options;
        self
    }

    /// The API client.
    pub fn client(&self) -> &GmailClient {
        &self.client
    }

    /// The options.
    pub fn options(&self) -> EngineOptions {
        self.options
    }

    /// The account's address and current history id (`users.getProfile`;
    /// also the account test).
    pub async fn profile(&self) -> Result<crate::wire::Profile, GmailError> {
        self.client.profile().await
    }

    async fn fetch_label_table(&self) -> Result<LabelTable, GmailError> {
        let labels = self.client.list_labels().await?;
        Ok(LabelTable::new(labels.iter().map(label_info)))
    }

    /// The cached label table (loaded on first use; `refresh` reloads it).
    pub async fn label_table(&self, refresh: bool) -> Result<LabelTable, GmailError> {
        let mut guard = self.labels.lock().await;
        if refresh || guard.is_none() {
            *guard = Some(self.fetch_label_table().await?);
        }
        Ok(guard.clone().unwrap_or_default())
    }

    /// The table for mapping `label_ids`: reloaded once when some ids are
    /// unknown (a label created on the server since).
    async fn table_for<'a>(
        &self,
        label_ids: impl IntoIterator<Item = &'a [String]>,
    ) -> Result<LabelTable, GmailError> {
        let table = self.label_table(false).await?;
        if label_ids.into_iter().any(|ids| table.has_unknown(ids)) {
            return self.label_table(true).await;
        }
        Ok(table)
    }

    /// Every label of the account from the server: system labels first,
    /// then user labels by name. Refreshes the cache.
    pub async fn labels(&self) -> Result<Vec<GmailLabelInfo>, GmailError> {
        let table = self.label_table(true).await?;
        let mut labels: Vec<GmailLabelInfo> = table.labels().cloned().collect();
        labels.sort_by(|a, b| {
            (
                a.kind == GmailLabelKind::User,
                a.settings.name.to_lowercase(),
            )
                .cmp(&(
                    b.kind == GmailLabelKind::User,
                    b.settings.name.to_lowercase(),
                ))
        });
        Ok(labels)
    }

    /// One label with its message counts (`labels.get`).
    pub async fn label(&self, id: &str) -> Result<GmailLabelInfo, GmailError> {
        Ok(label_info(&self.client.get_label(id).await?))
    }

    /// Creates a user label (replaying a create is safe: when the name
    /// exists already, that label is returned).
    pub async fn create_label(
        &self,
        settings: &GmailLabelSettings,
    ) -> Result<GmailLabelInfo, GmailError> {
        let mut guard = self.labels.lock().await;
        match self.client.create_label(settings).await {
            Ok(label) => {
                let info = label_info(&label);
                if let Some(table) = guard.as_mut() {
                    table.insert(info.clone());
                }
                Ok(info)
            }
            Err(GmailError::Conflict(message)) => {
                let table = self.fetch_label_table().await?;
                let found = table
                    .user_label_for_tag(settings.name.trim())
                    .and_then(|id| table.get(id))
                    .cloned();
                *guard = Some(table);
                found.ok_or(GmailError::Conflict(message))
            }
            Err(e) => Err(e),
        }
    }

    /// Changes a user label's name, colour or visibility.
    pub async fn update_label(
        &self,
        id: &str,
        settings: &GmailLabelSettings,
    ) -> Result<GmailLabelInfo, GmailError> {
        let info = label_info(&self.client.patch_label(id, settings).await?);
        if let Some(table) = self.labels.lock().await.as_mut() {
            table.insert(info.clone());
        }
        Ok(info)
    }

    /// Deletes a user label (an already deleted one is fine).
    pub async fn delete_label(&self, id: &str) -> Result<(), GmailError> {
        match self.client.delete_label(id).await {
            Ok(()) | Err(GmailError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        if let Some(table) = self.labels.lock().await.as_mut() {
            table.remove(id);
        }
        Ok(())
    }

    /// The label ids of Nail tags. Unknown tags become new user labels
    /// when `create` is set (names Gmail would refuse are skipped), else
    /// they are left out.
    pub async fn label_ids_for_tags(
        &self,
        tags: &[String],
        create: bool,
    ) -> Result<Vec<String>, GmailError> {
        let mut out = Vec::new();
        let mut missing = Vec::new();
        {
            let table = self.label_table(false).await?;
            for tag in tags {
                match table.user_label_for_tag(tag) {
                    Some(id) => out.push(id.to_owned()),
                    None => missing.push(tag.clone()),
                }
            }
        }
        if !missing.is_empty() {
            // Maybe created elsewhere since the table was loaded.
            let table = self.label_table(true).await?;
            for tag in missing {
                if let Some(id) = table.user_label_for_tag(&tag) {
                    out.push(id.to_owned());
                    continue;
                }
                if !create {
                    continue;
                }
                let settings = GmailLabelSettings::named(tag.trim());
                if !settings.issues().is_empty() {
                    tracing::warn!(%tag, "tag is not a valid Gmail label name; not synced");
                    continue;
                }
                out.push(self.create_label(&settings).await?.id);
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// The label change of a Nail flag change: `\Seen` / `\Flagged` and
    /// tags (new tags create labels).
    pub async fn label_change_for(&self, change: &FlagChange) -> Result<LabelChange, GmailError> {
        let mut out = LabelChange::from_flags(change);
        if !change.add_tags.is_empty() {
            out.add
                .extend(self.label_ids_for_tags(&change.add_tags, true).await?);
        }
        if !change.remove_tags.is_empty() {
            out.remove
                .extend(self.label_ids_for_tags(&change.remove_tags, false).await?);
        }
        Ok(out)
    }

    /// One message's metadata as an API engine reports it.
    pub async fn message(&self, id: &str) -> Result<RemoteMessage, GmailError> {
        let message = self.client.get_message(id, Format::Minimal, &[]).await?;
        let table = self.table_for([message.label_ids.as_slice()]).await?;
        Ok(remote_message(&message, &table))
    }

    /// Every message id of the mailbox (Spam and Trash included): ids
    /// only, `messages.list` pages of 500 without fetching anything else.
    /// Merging a resumed full listing needs the whole set.
    pub async fn all_message_ids(&self) -> Result<HashSet<String>, GmailError> {
        let mut out = HashSet::new();
        let mut token: Option<String> = None;
        loop {
            let list = self
                .client
                .list_messages(&ListQuery {
                    include_spam_trash: true,
                    max_results: Some(500),
                    page_token: token.take(),
                    ..ListQuery::default()
                })
                .await?;
            out.extend(list.messages.into_iter().map(|m| m.id));
            match list.next_page_token {
                Some(next) => token = Some(next),
                None => return Ok(out),
            }
        }
    }

    /// The message ids of a thread, oldest first.
    pub async fn thread_message_ids(&self, thread_id: &str) -> Result<Vec<String>, GmailError> {
        let thread = self.client.get_thread(thread_id, Format::Minimal).await?;
        Ok(thread.messages.into_iter().map(|m| m.id).collect())
    }

    /// Fetches listed messages (`format=minimal`, batched). Messages
    /// deleted meanwhile are returned separately; chat transcripts are
    /// skipped; any other failure fails the call (nothing is lost: the
    /// caller keeps its cursor).
    async fn fetch_listed(
        &self,
        ids: &[String],
    ) -> Result<(Vec<Message>, Vec<String>), GmailError> {
        if ids.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let answers = self
            .client
            .batch_get(ids, Format::Minimal, self.options.batch_size)
            .await?;
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for (id, answer) in ids.iter().zip(answers) {
            match answer {
                Ok(message) if is_chat(&message.label_ids) => {}
                Ok(message) => found.push(message),
                Err(GmailError::NotFound(_)) => missing.push(id.clone()),
                Err(e) => return Err(e),
            }
        }
        Ok((found, missing))
    }

    async fn to_remote(&self, messages: &[Message]) -> Result<Vec<RemoteMessage>, GmailError> {
        let table = self
            .table_for(messages.iter().map(|m| m.label_ids.as_slice()))
            .await?;
        Ok(messages.iter().map(|m| remote_message(m, &table)).collect())
    }

    async fn full_page(
        &self,
        page: Option<&str>,
        since: Option<i64>,
    ) -> Result<FullSyncPage, GmailError> {
        // The cursor is taken before listing: changes during the listing
        // come again through history.
        let cursor = match page {
            None => Some(self.client.profile().await?.history_id),
            Some(_) => None,
        };
        let list = self
            .client
            .list_messages(&ListQuery {
                q: since.map(after_query),
                include_spam_trash: true,
                max_results: Some(self.options.page_size),
                page_token: page.map(str::to_owned),
                ..ListQuery::default()
            })
            .await?;
        let ids: Vec<String> = list.messages.into_iter().map(|m| m.id).collect();
        let (found, _) = self.fetch_listed(&ids).await?;
        Ok(FullSyncPage {
            cursor,
            messages: self.to_remote(&found).await?,
            next_page: list.next_page_token,
        })
    }

    async fn delta(&self, cursor: &str) -> Result<DeltaOutcome, GmailError> {
        if cursor.parse::<u64>().is_err() {
            tracing::warn!(cursor, "not a Gmail history id; resyncing");
            return Ok(DeltaOutcome::CursorExpired);
        }
        let mut records: Vec<History> = Vec::new();
        let mut token: Option<String> = None;
        let mut pages = 0;
        let (next_cursor, more) = loop {
            let page = match self
                .client
                .list_history(cursor, token.as_deref(), self.options.history_page_size)
                .await?
            {
                HistoryPage::Expired => return Ok(DeltaOutcome::CursorExpired),
                HistoryPage::Page(page) => page,
            };
            records.extend(page.history);
            pages += 1;
            match page.next_page_token {
                None => break (page.history_id, false),
                Some(next) if pages >= self.options.history_pages_per_call.max(1) => {
                    let _ = next;
                    let last = records
                        .last()
                        .map_or_else(|| cursor.to_owned(), |r| r.id.clone());
                    break (last, true);
                }
                Some(next) => token = Some(next),
            }
        };
        let fold = fold_history(&records);
        let mut removed = fold.removed;
        let (added_messages, missing) = self.fetch_listed(&fold.added).await?;
        removed.extend(missing);

        // Changed messages whose labels the records did not carry.
        let unknown: Vec<String> = fold
            .changed
            .iter()
            .filter(|(_, labels)| labels.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        let (fetched, gone) = self.fetch_listed(&unknown).await?;
        removed.extend(gone.iter().cloned());
        let mut changed_labels: Vec<(String, String, Vec<String>)> = Vec::new();
        for (id, labels) in &fold.changed {
            let thread = fold.threads.get(id).cloned().unwrap_or_default();
            match labels {
                Some(labels) => changed_labels.push((id.clone(), thread, labels.clone())),
                None => {
                    if let Some(m) = fetched.iter().find(|m| &m.id == id) {
                        changed_labels.push((id.clone(), m.thread_id.clone(), m.label_ids.clone()));
                    }
                }
            }
        }
        changed_labels.retain(|(_, _, labels)| !is_chat(labels));
        let table = self
            .table_for(
                changed_labels
                    .iter()
                    .map(|(_, _, l)| l.as_slice())
                    .chain(added_messages.iter().map(|m| m.label_ids.as_slice())),
            )
            .await?;
        let mut seen = HashSet::new();
        removed.retain(|id| seen.insert(id.clone()));
        Ok(DeltaOutcome::Delta(SyncDelta {
            cursor: next_cursor,
            added: added_messages
                .iter()
                .map(|m| remote_message(m, &table))
                .collect(),
            changed: changed_labels
                .iter()
                .map(|(id, thread, labels)| remote_change(id, thread, labels, &table))
                .collect(),
            removed,
            more,
        }))
    }

    async fn replay(&self, write: &RemoteWrite) -> Result<WriteOutcome, EngineError> {
        match write {
            RemoteWrite::SetFlags { remote_id, change } => {
                let labels = self.label_change_for(change).await?;
                if labels.is_empty() {
                    return Ok(WriteOutcome::Done { remote_id: None });
                }
                gone_on_not_found(
                    self.client
                        .modify_message(remote_id, &labels)
                        .await
                        .map(drop),
                )
            }
            RemoteWrite::Move {
                remote_id,
                from_folder,
                to_folder,
            } => self.move_message(remote_id, from_folder, to_folder).await,
            RemoteWrite::Trash { remote_id } => {
                gone_on_not_found(self.client.trash_message(remote_id).await.map(drop))
            }
            RemoteWrite::Delete { remote_id } => {
                gone_on_not_found(self.client.delete_message(remote_id).await)
            }
            RemoteWrite::Append { folder, raw, flags } => {
                let target = GmailFolder::from_path(folder).ok_or_else(|| {
                    GmailError::Rejected(format!("unknown Gmail folder {folder:?}"))
                })?;
                let headers = HeaderInfo::parse(raw);
                if let Some(message_id) = &headers.message_id
                    && let Some(existing) = self.client.find_by_message_id(message_id).await?
                {
                    return Ok(WriteOutcome::Done {
                        remote_id: Some(existing.id),
                    });
                }
                let thread = self.thread_for(&headers).await;
                let id = if target == GmailFolder::Drafts {
                    self.client
                        .create_draft(raw, thread.as_deref())
                        .await?
                        .message
                        .id
                } else {
                    self.client
                        .insert_message(raw, &labels_for_insert(target, *flags), thread.as_deref())
                        .await?
                        .id
                };
                Ok(WriteOutcome::Done {
                    remote_id: Some(id),
                })
            }
        }
    }

    async fn move_message(
        &self,
        remote_id: &str,
        from_folder: &str,
        to_folder: &str,
    ) -> Result<WriteOutcome, EngineError> {
        let unsupported = || GmailError::MoveUnsupported {
            from: from_folder.to_owned(),
            to: to_folder.to_owned(),
        };
        let from = GmailFolder::from_path(from_folder).ok_or_else(unsupported)?;
        let to = GmailFolder::from_path(to_folder).ok_or_else(unsupported)?;
        let mut change = from.move_change(to).ok_or_else(unsupported)?;
        if change.is_empty() {
            return Ok(WriteOutcome::Done { remote_id: None });
        }
        if to == GmailFolder::Trash {
            return gone_on_not_found(self.client.trash_message(remote_id).await.map(drop));
        }
        if from == GmailFolder::Trash && change.remove.iter().any(|l| l == system::TRASH) {
            if let Err(e) = self.client.untrash_message(remote_id).await {
                return gone_on_not_found(Err(e));
            }
            change.remove.retain(|l| l != system::TRASH);
        }
        if change.is_empty() {
            return Ok(WriteOutcome::Done { remote_id: None });
        }
        gone_on_not_found(
            self.client
                .modify_message(remote_id, &change)
                .await
                .map(drop),
        )
    }

    /// The Gmail thread of the message `headers` replies to, if it is in
    /// the mailbox (best effort: errors only lose the threading).
    async fn thread_for(&self, headers: &HeaderInfo) -> Option<String> {
        for parent in headers.parents() {
            match self.client.find_by_message_id(parent).await {
                Ok(Some(found)) if !found.thread_id.is_empty() => return Some(found.thread_id),
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!(error = %e, "no thread for the reply");
                    return None;
                }
            }
        }
        None
    }

    /// Sends a finished message (`messages.send`) and returns Gmail's
    /// record of it (id, thread, `SENT`). Replies go into the thread of
    /// their original. Gmail files the message in Sent itself.
    pub async fn send_raw(&self, raw: &[u8]) -> Result<Message, GmailError> {
        let headers = HeaderInfo::parse(raw);
        let thread = self.thread_for(&headers).await;
        self.client.send_message(raw, thread.as_deref()).await
    }

    async fn send_envelope(
        &self,
        envelope: &Envelope,
        message: &[u8],
    ) -> Result<SendReceipt, GmailError> {
        let raw = with_envelope_recipients(message, &envelope.to)?;
        let sent = self.send_raw(&raw).await?;
        Ok(SendReceipt {
            response: format!("gmail id {} thread {}", sent.id, sent.thread_id),
        })
    }
}

/// The headers the engine needs from a raw message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HeaderInfo {
    pub message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    /// Lower-cased addresses of `To`, `Cc` and `Bcc`.
    pub recipients: HashSet<String>,
}

fn id_list(value: &mail_parser::HeaderValue<'_>) -> Vec<String> {
    match value {
        mail_parser::HeaderValue::Text(id) => vec![id.to_string()],
        mail_parser::HeaderValue::TextList(ids) => ids.iter().map(ToString::to_string).collect(),
        _ => Vec::new(),
    }
}

impl HeaderInfo {
    pub(crate) fn parse(raw: &[u8]) -> Self {
        let Some(message) = mail_parser::MessageParser::default().parse_headers(raw) else {
            return Self::default();
        };
        let mut recipients = HashSet::new();
        for list in [message.to(), message.cc(), message.bcc()]
            .into_iter()
            .flatten()
        {
            for addr in list.iter() {
                if let Some(a) = &addr.address {
                    recipients.insert(a.to_lowercase());
                }
            }
        }
        Self {
            message_id: message.message_id().map(str::to_owned),
            in_reply_to: id_list(message.in_reply_to()),
            references: id_list(message.references()),
            recipients,
        }
    }

    /// The messages this one replies to, closest first.
    pub(crate) fn parents(&self) -> impl Iterator<Item = &str> {
        self.in_reply_to
            .iter()
            .chain(self.references.iter().rev())
            .map(String::as_str)
            .take(3)
    }
}

/// `message` with a `Bcc` header for envelope recipients that are not in
/// its `To` / `Cc` / `Bcc` headers (Gmail sends to the header recipients
/// only, and removes `Bcc` before delivery).
pub fn with_envelope_recipients(
    message: &[u8],
    recipients: &[String],
) -> Result<Vec<u8>, GmailError> {
    let headers = HeaderInfo::parse(message);
    let mut missing: Vec<&str> = Vec::new();
    for r in recipients {
        let r = r.trim();
        if r.is_empty() || headers.recipients.contains(&r.to_lowercase()) || missing.contains(&r) {
            continue;
        }
        let plain = r.contains('@')
            && !r
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || ",;<>\"()[]\\".contains(c));
        if !plain {
            return Err(GmailError::Rejected(format!("invalid recipient {r:?}")));
        }
        missing.push(r);
    }
    if missing.is_empty() {
        return Ok(message.to_vec());
    }
    let mut out = format!("Bcc: {}\r\n", missing.join(", ")).into_bytes();
    out.extend_from_slice(message);
    Ok(out)
}

impl MailApiEngine for GmailEngine {
    fn folders(&self) -> BoxFuture<'_, Result<Vec<RemoteFolder>, EngineError>> {
        Box::pin(async { Ok(remote_folders()) })
    }

    fn full_sync_page<'a>(
        &'a self,
        page: Option<&'a str>,
        since: Option<i64>,
    ) -> BoxFuture<'a, Result<FullSyncPage, EngineError>> {
        Box::pin(async move { Ok(self.full_page(page, since).await?) })
    }

    fn changes<'a>(&'a self, cursor: &'a str) -> BoxFuture<'a, Result<DeltaOutcome, EngineError>> {
        Box::pin(async move { Ok(self.delta(cursor).await?) })
    }

    fn fetch_raw<'a>(&'a self, remote_id: &'a str) -> BoxFuture<'a, Result<Vec<u8>, EngineError>> {
        Box::pin(async move { Ok(self.client.get_raw(remote_id).await?) })
    }

    fn write<'a>(
        &'a self,
        write: &'a RemoteWrite,
    ) -> BoxFuture<'a, Result<WriteOutcome, EngineError>> {
        Box::pin(self.replay(write))
    }
}

impl OutgoingTransport for GmailEngine {
    fn send<'a>(
        &'a self,
        envelope: &'a Envelope,
        message: &'a [u8],
    ) -> BoxFuture<'a, Result<SendReceipt, EngineError>> {
        Box::pin(async move { Ok(self.send_envelope(envelope, message).await?) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_info() {
        let raw = b"From: a@x\r\nTo: \"Bob\" <Bob@Example.org>, c@x\r\nCc: d@x\r\nMessage-ID: <m1@x>\r\nIn-Reply-To: <p1@x>\r\nReferences: <p0@x> <p1@x>\r\nSubject: s\r\n\r\nbody";
        let info = HeaderInfo::parse(raw);
        assert_eq!(info.message_id.as_deref(), Some("m1@x"));
        assert!(info.recipients.contains("bob@example.org"));
        assert!(info.recipients.contains("d@x"));
        let parents: Vec<&str> = info.parents().collect();
        assert_eq!(parents[0], "p1@x");
        assert!(parents.contains(&"p0@x"));
    }

    #[test]
    fn envelope_recipients_become_bcc() {
        let raw = b"To: bob@example.org\r\nSubject: s\r\n\r\nbody";
        let same = with_envelope_recipients(raw, &["BOB@example.org".into()]).unwrap();
        assert_eq!(same, raw);
        let out = with_envelope_recipients(
            raw,
            &[
                "bob@example.org".into(),
                "hidden@example.org".into(),
                "hidden@example.org".into(),
            ],
        )
        .unwrap();
        assert!(out.starts_with(b"Bcc: hidden@example.org\r\nTo: bob"));
        assert!(matches!(
            with_envelope_recipients(raw, &["x@y\r\nX-Evil: 1".into()]),
            Err(GmailError::Rejected(_))
        ));
        assert!(matches!(
            with_envelope_recipients(raw, &["no-at-sign".into()]),
            Err(GmailError::Rejected(_))
        ));
    }
}
