//! The MCP server handler: resources, subscriptions, tools, grants and
//! audit (design.md, "MCP server").
//!
//! Every request is made on behalf of one registered client: the HTTP
//! layer ([`crate::http`]) authenticates the bearer token and hands the
//! client to the handler through the request's extensions; a stdio server
//! started for one client ([`ClientSource::Fixed`]) looks it up by id. The
//! grant is read from the store on every call, so revoking or narrowing it
//! takes effect at once. Every `resources/read`, `resources/subscribe`,
//! `subscriptions/listen`, `tools/call` and `prompts/get` writes one audit
//! entry (a write that waits for a confirmation writes it when it ends).
//!
//! M4a (ADR 0079): calls count against the client's rate limits
//! ([`crate::limits`]); write tools (`crate::writes`) and prompts
//! (`crate::prompts`).

use std::collections::BTreeSet;
use std::sync::Arc;

use nail_api::mcp::{McpCallKind, McpClient, McpOutcome, McpScope};
use nail_api::{
    AccountId, ApiError, ErrorCode, Event, FolderId, McpClientId, MessageId, MessageSummary,
    UiContext,
};
use nail_store::{McpAuditRecord, Store};
use rmcp::ErrorData;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Extensions, GetPromptRequestParams, GetPromptResponse,
    Implementation, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceTemplate, ServerCapabilities, ServerConfig,
    SubscribeRequestParams, SubscriptionFilter, UnsubscribeRequestParams,
};
use rmcp::service::{RequestContext, RoleServer, SubscriptionContext};
use serde_json::{Value, json};

use crate::access::{Access, Denial};
use crate::broker::{ConfirmationBroker, StoreBroker};
use crate::content::{
    UNTRUSTED_NOTICE, json_resource, untrusted_blob_resource, untrusted_json_resource,
};
use crate::data::McpData;
use crate::limits::{Limited, RateLimiter, Window};
use crate::subscriptions::{Hub, SessionGuard};
use crate::uri::{self, ResourceRef};
use crate::writes::{WriteSettings, WriteState};

/// The authenticated client of an HTTP request (set by the auth layer in
/// the request's extensions).
#[derive(Debug, Clone)]
pub struct AuthedClient(pub McpClient);

/// How the handler learns which client is calling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientSource {
    /// From the HTTP request ([`AuthedClient`] in the request parts).
    Http,
    /// Always this client (a stdio server started for one client).
    Fixed(McpClientId),
}

/// Why a call failed.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Refused by the grant.
    Denied(Denial),
    /// A data error (not found, invalid query, …).
    Api(ApiError),
    /// Over one of the client's rate limits (M4a).
    Limited(Limited),
}

impl From<Denial> for Failure {
    fn from(denial: Denial) -> Self {
        Self::Denied(denial)
    }
}

impl From<ApiError> for Failure {
    fn from(err: ApiError) -> Self {
        Self::Api(err)
    }
}

impl Failure {
    /// Audit outcome and detail.
    pub(crate) fn audit(&self) -> (McpOutcome, Option<String>) {
        match self {
            Self::Denied(denial) => (McpOutcome::Denied, Some(denial.to_string())),
            Self::Api(err) if err.code == ErrorCode::RateLimited => {
                (McpOutcome::Denied, Some(err.to_string()))
            }
            Self::Api(err) => (McpOutcome::Error, Some(err.to_string())),
            Self::Limited(limited) => (McpOutcome::Denied, Some(limited.to_string())),
        }
    }

    /// As a JSON-RPC error (resources, subscriptions).
    pub(crate) fn to_error(&self) -> ErrorData {
        match self {
            Self::Denied(denial) => ErrorData::invalid_request(
                format!("access denied: {denial}"),
                Some(json!({"nail": "denied", "reason": denial.to_string()})),
            ),
            Self::Api(err) if err.code == ErrorCode::NotFound => {
                ErrorData::resource_not_found(err.message.clone(), None)
            }
            Self::Api(err) if err.code == ErrorCode::InvalidRequest => {
                ErrorData::invalid_params(err.message.clone(), None)
            }
            Self::Api(err) if err.code == ErrorCode::RateLimited => ErrorData::invalid_request(
                err.message.clone(),
                Some(json!({"nail": "rate_limited"})),
            ),
            Self::Api(err) => ErrorData::internal_error(err.to_string(), None),
            Self::Limited(limited) => ErrorData::invalid_request(
                format!("rate limited: {limited}"),
                Some(json!({
                    "nail": "rate_limited",
                    "limit": limited.window.name(),
                    "retry_after_secs": limited.retry_after_secs,
                })),
            ),
        }
    }
}

/// Current time, Unix seconds.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

/// State shared by every session of one server.
pub(crate) struct Shared<D> {
    pub store: Store,
    pub data: Arc<D>,
    pub hub: Arc<Hub>,
    /// Confirmations (the core's, or the store-backed one).
    pub broker: Arc<dyn ConfirmationBroker>,
    /// Per-client rate limits (the core's, shared with plug-in writes for
    /// clients, or the server's own).
    pub limiter: Arc<RateLimiter>,
    /// Writes waiting for a decision, MRTR continuations, idempotency keys.
    pub writes: WriteState,
    /// Confirmation timeout and polling.
    pub settings: WriteSettings,
}

impl<D: McpData> Shared<D> {
    /// The state of one server over `store` and `data`.
    pub(crate) fn new(store: Store, data: Arc<D>, settings: WriteSettings) -> Self {
        let broker = data.confirmations().unwrap_or_else(|| {
            let sink = data.clone();
            Arc::new(StoreBroker::new(
                store.clone(),
                Arc::new(move |event| sink.emit(event)),
                settings.poll_interval,
            ))
        });
        let limiter = data.rate_limiter().unwrap_or_default();
        Self {
            store,
            data,
            hub: Arc::new(Hub::default()),
            broker,
            limiter,
            writes: WriteState::default(),
            settings,
        }
    }

    /// Counts a call against the client's calls per minute.
    pub(crate) fn count_request(&self, client: &McpClient) -> Result<(), Failure> {
        self.limiter
            .check(client, Window::Requests, 1)
            .map_err(Failure::Limited)
    }

    /// Appends an audit entry; failures are logged, never returned.
    pub(crate) async fn audit(
        &self,
        client: Option<&McpClient>,
        kind: McpCallKind,
        name: &str,
        outcome: McpOutcome,
        detail: Option<String>,
    ) {
        let record = McpAuditRecord {
            client_id: client.map(|c| c.id.clone()),
            client_name: client.map(|c| c.name.clone()).unwrap_or_default(),
            at: now(),
            kind,
            name: name.to_owned(),
            outcome,
            detail,
        };
        if let Err(err) = self.store.call(move |s| s.append_mcp_audit(&record)).await {
            tracing::warn!(%err, "cannot write the MCP audit log");
        }
    }

    /// Audits the result of a call and passes it on.
    pub(crate) async fn audited<T>(
        &self,
        client: &McpClient,
        kind: McpCallKind,
        name: &str,
        result: Result<T, Failure>,
    ) -> Result<T, Failure> {
        let (outcome, detail) = match &result {
            Ok(_) => (McpOutcome::Allowed, None),
            Err(failure) => failure.audit(),
        };
        self.audit(Some(client), kind, name, outcome, detail).await;
        result
    }

    /// The client's grant, resolved against the current virtual accounts.
    /// A resource request counted against the client's limit: the resource
    /// `uri` names and the client's access.
    async fn counted_resource(
        &self,
        client: &McpClient,
        uri: &str,
    ) -> Result<(ResourceRef, Access), Failure> {
        self.count_request(client)?;
        let resource = ResourceRef::parse(uri)
            .ok_or_else(|| Failure::Api(ApiError::not_found(format!("unknown resource {uri}"))))?;
        Ok((resource, self.access(client).await))
    }

    pub(crate) async fn access(&self, client: &McpClient) -> Access {
        if client.grant.is_empty() {
            return Access::none();
        }
        let virtuals = match self.data.virtual_accounts().await {
            Ok(virtuals) => virtuals,
            Err(err) => {
                // Without the member lists a grant naming a virtual account
                // covers only the virtual account itself (never more).
                tracing::warn!(%err, "cannot list virtual accounts for an MCP grant");
                Vec::new()
            }
        };
        Access::of(client, &virtuals)
    }

    /// The current state of a client by id (`None` when unknown or
    /// revoked).
    pub(crate) async fn active_client(&self, id: &McpClientId) -> Option<McpClient> {
        let id = id.clone();
        match self.store.call(move |s| s.get_mcp_client(&id)).await {
            Ok(client) if client.revoked_at.is_none() => Some(client),
            _ => None,
        }
    }

    /// Checks that `folder` belongs to a granted account and is not
    /// excluded.
    pub(crate) async fn require_folder(
        &self,
        access: &Access,
        folder: &FolderId,
    ) -> Result<(), Failure> {
        access.require(McpScope::Read)?;
        if !access.allows_folder(folder) {
            return Err(Denial::Folder(folder.clone()).into());
        }
        if access.all_accounts() {
            return Ok(());
        }
        for account in self.data.accounts().await? {
            if !access.allows_account(&account.id) {
                continue;
            }
            if let Ok(folders) = self.data.folders(&account.id).await
                && folders.iter().any(|f| &f.id == folder)
            {
                return Ok(());
            }
        }
        Err(Denial::Folder(folder.clone()).into())
    }

    /// A message's summary, if the grant allows reading it (and its content:
    /// encrypted messages need `encrypted`).
    pub(crate) async fn readable_message(
        &self,
        access: &Access,
        id: &MessageId,
    ) -> Result<MessageSummary, Failure> {
        access.require(McpScope::Read)?;
        let summary = self.data.message_summary(id).await?;
        access.require_message(&summary)?;
        if self.data.is_encrypted(id).await? {
            access.require(McpScope::Encrypted)?;
        }
        Ok(summary)
    }

    /// `nail://accounts`
    pub(crate) async fn accounts_json(&self, access: &Access) -> Result<Value, Failure> {
        access.require(McpScope::Read)?;
        let accounts: Vec<_> = self
            .data
            .accounts()
            .await?
            .into_iter()
            .filter(|a| access.allows_account(&a.id))
            .map(|a| {
                let mut value = json!(a);
                value["folders_uri"] = json!(uri::folders(&a.id));
                value
            })
            .collect();
        Ok(json!({ "accounts": accounts }))
    }

    /// `nail://accounts/{id}/folders`
    pub(crate) async fn folders_json(
        &self,
        access: &Access,
        account: &AccountId,
    ) -> Result<Value, Failure> {
        access.require_account(account)?;
        let folders: Vec<_> = self
            .data
            .folders(account)
            .await?
            .into_iter()
            .filter(|f| access.allows_folder(&f.id))
            .map(|f| {
                let mut value = json!(f);
                value["messages_uri"] = json!(uri::folder_messages(&f.id));
                value
            })
            .collect();
        Ok(json!({ "account_id": account, "folders": folders }))
    }

    /// `nail://folders/{id}/messages`
    pub(crate) async fn folder_messages_json(
        &self,
        access: &Access,
        folder: &FolderId,
        offset: u32,
        limit: u32,
    ) -> Result<Value, Failure> {
        self.require_folder(access, folder).await?;
        let page = self.data.messages(folder, offset, limit).await?;
        let returned = u32::try_from(page.messages.len()).unwrap_or(u32::MAX);
        let messages: Vec<_> = page
            .messages
            .iter()
            .filter(|m| access.allows_message(m))
            .map(summary_json)
            .collect();
        let hidden = returned as usize - messages.len();
        let next = page.offset.saturating_add(returned);
        Ok(json!({
            "folder_id": folder,
            "offset": page.offset,
            "total": page.total,
            "hidden": hidden,
            "next_offset": (next < page.total).then_some(next),
            "messages": messages,
        }))
    }

    /// `nail://messages/{id}` and `get_message`
    pub(crate) async fn message_json(
        &self,
        access: &Access,
        id: &MessageId,
    ) -> Result<Value, Failure> {
        let summary = self.readable_message(access, id).await?;
        let body = self.data.message_body(id).await?;
        let attachments: Vec<_> = body
            .attachments
            .iter()
            .map(|a| {
                let mut value = json!(a);
                value["uri"] = json!(ResourceRef::Attachment(id.clone(), a.index).uri());
                value
            })
            .collect();
        Ok(json!({
            "id": summary.id,
            "account_id": summary.account_id,
            "folder_id": summary.folder_id,
            "thread_id": summary.thread_id,
            "flags": summary.flags,
            "tags": summary.tags,
            "date": summary.date,
            // M3c: Exchange data (Focused Inbox, conversation, mailbox).
            "exchange": summary.exchange,
            "headers": body.headers,
            "text": body.text,
            "html": body.sanitized_html,
            "attachments": attachments,
            "raw_uri": ResourceRef::Raw(id.clone()).uri(),
        }))
    }

    /// `nail://threads/{id}` and `get_thread`
    pub(crate) async fn thread_json(
        &self,
        access: &Access,
        id: &nail_api::ThreadId,
    ) -> Result<Value, Failure> {
        access.require(McpScope::Read)?;
        let thread = self.data.thread(id).await?;
        let total = thread.messages.len();
        let visible: Vec<&MessageSummary> = thread
            .messages
            .iter()
            .filter(|m| access.allows_message(m))
            .collect();
        let Some(first) = thread.messages.first() else {
            return Ok(json!({ "thread_id": id, "messages": [] }));
        };
        if visible.is_empty() {
            return Err(access.require_message(first).err().map_or_else(
                || Failure::Api(ApiError::not_found("thread")),
                Failure::Denied,
            ));
        }
        let mut value = json!({
            "thread_id": id,
            "messages": visible.iter().map(|m| summary_json(m)).collect::<Vec<_>>(),
        });
        // The thread summary aggregates every message. A restricted grant
        // gets the aggregate of what it may read instead (and no count of
        // hidden messages), so nothing tells whether the thread continues
        // in an account or folder it may not see.
        if access.all_accounts() && !access.excludes_folders() && visible.len() == total {
            value["subject"] = json!(thread.thread.subject);
            value["participants"] = json!(thread.thread.participants);
        } else {
            // P2 (ADR 0148): the same shape as the full answer (`{name,
            // email, me, unread}`): "me" from the thread's participants,
            // unread from the visible messages only.
            let mut participants: Vec<nail_api::ThreadParticipant> = Vec::new();
            for m in &visible {
                let Some(from) = m.from.as_ref() else {
                    continue;
                };
                let unread = !m.flags.seen;
                if let Some(known) = participants
                    .iter_mut()
                    .find(|p| p.address.email.eq_ignore_ascii_case(&from.email))
                {
                    known.unread |= unread;
                    continue;
                }
                let me = thread
                    .thread
                    .participants
                    .iter()
                    .any(|p| p.me && p.address.email.eq_ignore_ascii_case(&from.email));
                participants.push(nail_api::ThreadParticipant {
                    address: from.clone(),
                    me,
                    unread,
                });
            }
            value["subject"] = json!(visible[0].subject);
            value["participants"] = json!(participants);
        }
        Ok(value)
    }

    /// `nail://rules`: the rules see every account, so they need a grant
    /// for all accounts.
    pub(crate) async fn rules_text(&self, access: &Access) -> Result<String, Failure> {
        access.require(McpScope::Read)?;
        if !access.all_accounts() {
            return Err(Denial::AllAccounts.into());
        }
        Ok(self.data.rules_text().await?)
    }

    /// `nail://ui/context` and `get_ui_context`; tells the UI (indicator).
    pub(crate) async fn ui_context_json(
        &self,
        access: &Access,
        client: &McpClient,
    ) -> Result<Value, Failure> {
        access.require(McpScope::Context)?;
        let context: UiContext = self.data.ui_context().await?;
        let message = match &context.message_id {
            Some(id) => Some(match self.readable_message(access, id).await {
                Ok(summary) => {
                    let mut value = summary_json(&summary);
                    value["selected_text"] = json!(context.selected_text);
                    value
                }
                // The context grant shows what is open, not what the grant
                // otherwise hides.
                Err(_) => json!({ "id": id, "hidden": true }),
            }),
            None => None,
        };
        let mut selected = Vec::new();
        for id in context
            .selected_message_ids
            .iter()
            .take(uri::MAX_PAGE as usize)
        {
            if let Ok(summary) = self.data.message_summary(id).await
                && access.allows_message(&summary)
            {
                selected.push(id.clone());
            }
        }
        let draft = match &context.draft_id {
            Some(id) => Some(self.readable_draft(access, id, context.draft_cursor).await),
            None => None,
        };
        // Where the user is: only what the grant covers.
        let account_id = context
            .account_id
            .as_ref()
            .filter(|a| access.allows_account(a));
        let folder_id = match &context.folder_id {
            Some(folder) if self.require_folder(access, folder).await.is_ok() => Some(folder),
            _ => None,
        };
        let thread_id = match &context.thread_id {
            Some(thread) => match self.data.thread(thread).await {
                Ok(data) if data.messages.iter().any(|m| access.allows_message(m)) => Some(thread),
                _ => None,
            },
            None => None,
        };
        self.data.emit(Event::McpContextAccess {
            client_id: client.id.clone(),
            client_name: client.name.clone(),
            edit: false,
        });
        Ok(json!({
            "account_id": account_id,
            "folder_id": folder_id,
            "thread_id": thread_id,
            "message": message,
            "selected_message_ids": selected,
            "draft": draft,
        }))
    }

    /// The open draft for the UI context, if the grant covers it: its
    /// identity's account must be granted, and the message it replies to,
    /// forwards or redirects (whose text it may quote) must be readable.
    /// Otherwise it is named, not shown.
    async fn readable_draft(
        &self,
        access: &Access,
        id: &nail_api::DraftId,
        cursor: Option<u32>,
    ) -> Value {
        let hidden = json!({ "id": id, "hidden": true });
        let draft = match self.data.draft(id).await {
            Ok(draft) => draft,
            Err(err) => {
                tracing::debug!(%err, "open draft not readable");
                return hidden;
            }
        };
        let account = match self.data.identities().await {
            Ok(identities) => identities
                .into_iter()
                .find(|i| i.id == draft.identity_id)
                .map(|i| i.account_id),
            Err(err) => {
                tracing::debug!(%err, "cannot list identities");
                None
            }
        };
        if !account.is_some_and(|a| access.allows_account(&a)) {
            return hidden;
        }
        let original = draft.original_message();
        if let Some(original) = original
            && self.readable_message(access, &original).await.is_err()
        {
            return hidden;
        }
        json!({ "draft": draft, "cursor": cursor })
    }

    /// Reads one resource.
    pub(crate) async fn read(
        &self,
        access: &Access,
        client: &McpClient,
        resource: &ResourceRef,
        requested_uri: &str,
    ) -> Result<ReadResourceResult, Failure> {
        let uri = requested_uri;
        let contents = match resource {
            ResourceRef::Accounts => json_resource(uri, &self.accounts_json(access).await?),
            ResourceRef::Folders(account) => {
                json_resource(uri, &self.folders_json(access, account).await?)
            }
            ResourceRef::FolderMessages {
                folder,
                offset,
                limit,
            } => untrusted_json_resource(
                uri,
                self.folder_messages_json(access, folder, *offset, *limit)
                    .await?,
            ),
            ResourceRef::Message(id) => {
                untrusted_json_resource(uri, self.message_json(access, id).await?)
            }
            ResourceRef::Raw(id) => {
                self.readable_message(access, id).await?;
                access.require(McpScope::RawSource)?;
                let raw = self.data.raw_message(id).await?;
                untrusted_blob_resource(uri, "message/rfc822", &raw)
            }
            ResourceRef::Attachment(id, index) => {
                self.readable_message(access, id).await?;
                access.require(McpScope::Attachments)?;
                let attachment = self.data.attachment(id, *index).await?;
                untrusted_blob_resource(uri, &attachment.mime, &attachment.data)
            }
            ResourceRef::Thread(id) => {
                untrusted_json_resource(uri, self.thread_json(access, id).await?)
            }
            ResourceRef::Rules => {
                let text = self.rules_text(access).await?;
                rmcp::model::ResourceContents::text(text, uri)
            }
            ResourceRef::UiContext => {
                untrusted_json_resource(uri, self.ui_context_json(access, client).await?)
            }
            // M3a: calendars, contacts and tasks (read-only).
            ResourceRef::Calendars => {
                untrusted_json_resource(uri, self.calendars_json(access).await?)
            }
            ResourceRef::CalendarEvents { calendar, from, to } => untrusted_json_resource(
                uri,
                self.calendar_events_json(access, calendar, from.as_deref(), to.as_deref())
                    .await?,
            ),
            ResourceRef::Contacts => {
                untrusted_json_resource(uri, self.contacts_json(access).await?)
            }
            ResourceRef::Contact(id) => {
                untrusted_json_resource(uri, self.contact_json(access, id).await?)
            }
            ResourceRef::Tasks => untrusted_json_resource(
                uri,
                self.tasks_json(access, crate::pim::ListTasksArgs::default())
                    .await?,
            ),
        };
        Ok(ReadResourceResult::new(vec![contents]))
    }

    /// Checks a subscription request against the grant.
    pub(crate) async fn may_subscribe(
        &self,
        access: &Access,
        resource: &ResourceRef,
    ) -> Result<(), Failure> {
        match resource {
            ResourceRef::Accounts => Ok(access.require(McpScope::Read)?),
            ResourceRef::Folders(account) => Ok(access.require_account(account)?),
            ResourceRef::FolderMessages { folder, .. } => self.require_folder(access, folder).await,
            ResourceRef::UiContext => Ok(access.require(McpScope::Context)?),
            _ => Err(Failure::Api(ApiError::invalid_request(
                "this resource does not change and cannot be subscribed to",
            ))),
        }
    }

    /// Sends `notifications/resources/updated` for `resource` to every
    /// subscriber whose grant still allows it.
    pub(crate) async fn notify(&self, resource: &ResourceRef) {
        let uri = resource.uri();
        for due in self.hub.due(&uri) {
            let Some(client) = self.active_client(&due.client).await else {
                self.hub.remove(due.key);
                continue;
            };
            let access = self.access(&client).await;
            if self.may_subscribe(&access, resource).await.is_err() {
                continue;
            }
            if !due.sink.updated(&uri).await {
                self.hub.remove(due.key);
            }
        }
    }
}

/// A message summary for agents, with its resource URI.
pub(crate) fn summary_json(summary: &MessageSummary) -> Value {
    let mut value = json!(summary);
    value["uri"] = json!(uri::message(&summary.id));
    value
}

/// Server instructions sent at initialization.
pub const INSTRUCTIONS: &str = "Nail is the user's email client. Read accounts, folders, messages, \
threads, calendars, contacts, tasks and rules through the nail:// resources or the read tools. \
Write tools change mail, drafts, events, contacts, tasks and rules through Nail's operation queue \
(undoable). Deleting, sending, invitations and rule changes wait until the user confirms in Nail \
(or in your client's form where it offers one); a denied or expired confirmation changed nothing. \
Pass an idempotency_key to retry a write safely. Prompts: summarize_thread, triage_folder, \
draft_reply, prepare_meeting. Access is limited to what the user granted this client in Nail \
(Settings → MCP clients); a denied call names the missing grant. Calls are rate limited per client \
and every call is written to an audit log the user can see.";

/// The rmcp handler of one session.
#[derive(Clone)]
pub struct NailMcp<D: McpData> {
    shared: Arc<Shared<D>>,
    source: ClientSource,
    session: Arc<SessionGuard>,
}

impl<D: McpData> std::fmt::Debug for NailMcp<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NailMcp").finish_non_exhaustive()
    }
}

impl<D: McpData> NailMcp<D> {
    pub(crate) fn new(shared: Arc<Shared<D>>, source: ClientSource) -> Self {
        let session = Arc::new(SessionGuard {
            key: shared.hub.key(),
            hub: Arc::downgrade(&shared.hub),
        });
        Self {
            shared,
            source,
            session,
        }
    }

    /// A handler for one client over any transport (stdio, tests); the
    /// grant and audit log are the same as over HTTP.
    pub fn for_client(store: Store, data: Arc<D>, client: McpClientId) -> Self {
        Self::for_client_with(store, data, client, WriteSettings::default())
    }

    /// [`Self::for_client`] with other confirmation settings.
    pub fn for_client_with(
        store: Store,
        data: Arc<D>,
        client: McpClientId,
        settings: WriteSettings,
    ) -> Self {
        let shared = Arc::new(Shared::new(store, data, settings));
        Self::new(shared, ClientSource::Fixed(client))
    }

    /// The calling client, freshly loaded (refused when revoked).
    async fn client(&self, extensions: &Extensions) -> Result<McpClient, ErrorData> {
        let unauthenticated =
            || ErrorData::invalid_request("unauthenticated: no valid MCP client token", None);
        match &self.source {
            ClientSource::Http => extensions
                .get::<axum::http::request::Parts>()
                .and_then(|parts| parts.extensions.get::<AuthedClient>())
                .map(|authed| authed.0.clone())
                .ok_or_else(unauthenticated),
            ClientSource::Fixed(id) => self
                .shared
                .active_client(id)
                .await
                .ok_or_else(unauthenticated),
        }
    }
}

impl<D: McpData> rmcp::ServerHandler for NailMcp<D> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_resources_subscribe()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("nail", env!("CARGO_PKG_VERSION")))
        .with_instructions(format!("{INSTRUCTIONS} {UNTRUSTED_NOTICE}"))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let client = self.client(&context.extensions).await?;
        let access = self.shared.access(&client).await;
        let mut resources = Vec::new();
        if access.has(McpScope::Read) {
            resources.push(
                Resource::new(uri::ACCOUNTS, "accounts")
                    .with_description("Real and virtual accounts this client may read.")
                    .with_mime_type("application/json"),
            );
            if let Ok(accounts) = self.shared.data.accounts().await {
                for account in accounts.iter().filter(|a| access.allows_account(&a.id)) {
                    resources.push(
                        Resource::new(
                            uri::folders(&account.id),
                            format!("folders of {}", account.name),
                        )
                        .with_mime_type("application/json"),
                    );
                }
            }
            for (uri, name, description) in [
                (
                    uri::CALENDARS,
                    "calendars",
                    "Calendars and task lists this client may read (M3a).",
                ),
                (
                    uri::CONTACTS,
                    "contacts",
                    "Address books and contacts this client may read.",
                ),
                (uri::TASKS, "tasks", "Open tasks of the visible task lists."),
            ] {
                resources.push(
                    Resource::new(uri, name)
                        .with_description(description)
                        .with_mime_type("application/json"),
                );
            }
            if access.all_accounts() {
                resources.push(
                    Resource::new(uri::RULES, "rules")
                        .with_description("Filter rules in their rules.kdl text form.")
                        .with_mime_type("text/plain"),
                );
            }
        }
        if access.has(McpScope::Context) {
            resources.push(
                Resource::new(uri::UI_CONTEXT, "ui-context")
                    .with_description(
                        "What the user has on screen: the open message, selection and open draft. Subscribable.",
                    )
                    .with_mime_type("application/json"),
            );
        }
        Ok(ListResourcesResult::with_all_items(resources))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>>
    + rmcp::service::MaybeSendFuture
    + '_ {
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(
            uri::TEMPLATES
                .iter()
                .map(|(template, name, description)| {
                    ResourceTemplate::new(*template, *name).with_description(*description)
                })
                .collect(),
        )))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let client = self.client(&context.extensions).await?;
        let result = match self.shared.counted_resource(&client, &request.uri).await {
            Ok((resource, access)) => {
                self.shared
                    .read(&access, &client, &resource, &request.uri)
                    .await
            }
            Err(failure) => Err(failure),
        };
        self.shared
            .audited(&client, McpCallKind::Resource, &request.uri, result)
            .await
            .map(ReadResourceResponse::from)
            .map_err(|f| f.to_error())
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let client = self.client(&context.extensions).await?;
        let result = match self.shared.counted_resource(&client, &request.uri).await {
            Ok((resource, access)) => self
                .shared
                .may_subscribe(&access, &resource)
                .await
                .map(|()| resource),
            Err(failure) => Err(failure),
        };
        let resource = self
            .shared
            .audited(&client, McpCallKind::Subscribe, &request.uri, result)
            .await
            .map_err(|f| f.to_error())?;
        self.shared
            .hub
            .subscribe(self.session.key, &client.id, &context.peer, &resource.uri());
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.client(&context.extensions).await?;
        if let Some(resource) = ResourceRef::parse(&request.uri) {
            self.shared
                .hub
                .unsubscribe(self.session.key, &resource.uri());
        }
        Ok(())
    }

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        let uris: Vec<String> = requested
            .resource_subscriptions
            .iter()
            .flatten()
            .filter(|uri| {
                ResourceRef::parse(uri).is_some_and(|r| r.subscribable() && r.uri() == **uri)
            })
            .cloned()
            .collect();
        Some(
            SubscriptionFilter::builder()
                .resource_subscriptions(uris)
                .build(),
        )
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let client = self.client(&context.request_context().extensions).await?;
        let access = self.shared.access(&client).await;
        let mut allowed = BTreeSet::new();
        for uri in context.accepted().resource_subscriptions.iter().flatten() {
            let Some(resource) = ResourceRef::parse(uri) else {
                continue;
            };
            let result = self.shared.may_subscribe(&access, &resource).await;
            if self
                .shared
                .audited(&client, McpCallKind::Subscribe, uri, result)
                .await
                .is_ok()
            {
                allowed.insert(uri.clone());
            }
        }
        let key = self.shared.hub.key();
        self.shared
            .hub
            .listen(key, &client.id, context.sink().clone(), allowed);
        context.cancelled().await;
        self.shared.hub.remove(key);
        Ok(())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut tools = crate::tools::definitions();
        if let Ok(client) = self.client(&context.extensions).await {
            let access = self.shared.access(&client).await;
            tools.extend(crate::tools::plugin_definitions(
                self.shared.data.plugin_tools(),
                &access,
            ));
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let client = self.client(&context.extensions).await?;
        crate::tools::call(&self.shared, &client, request, &context).await
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, ErrorData>> + rmcp::service::MaybeSendFuture + '_
    {
        std::future::ready(Ok(ListPromptsResult::with_all_items(
            crate::prompts::definitions(self.shared.data.as_ref()),
        )))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        let client = self.client(&context.extensions).await?;
        crate::prompts::get(&self.shared, &client, request)
            .await
            .map(GetPromptResponse::from)
    }
}
