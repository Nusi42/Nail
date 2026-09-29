//! The MCP server handler: resources, subscriptions, tools, grants and
//! audit (design.md, "MCP server").
//!
//! Every request is made on behalf of one registered client: the HTTP
//! layer ([`crate::http`]) authenticates the bearer token and hands the
//! client to the handler through the request's extensions; a stdio server
//! started for one client ([`ClientSource::Fixed`]) looks it up by id. The
//! grant is read from the store on every call, so revoking or narrowing it
//! takes effect at once. Every `resources/read`, `resources/subscribe`,
//! `subscriptions/listen` and `tools/call` writes one audit entry.

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
    CallToolRequestParams, CallToolResponse, Extensions, Implementation,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceTemplate, ServerCapabilities, ServerConfig, SubscribeRequestParams, SubscriptionFilter,
    UnsubscribeRequestParams,
};
use rmcp::service::{RequestContext, RoleServer, SubscriptionContext};
use serde_json::{Value, json};

use crate::access::{Access, Denial};
use crate::content::{
    UNTRUSTED_NOTICE, json_resource, untrusted_blob_resource, untrusted_json_resource,
};
use crate::data::McpData;
use crate::subscriptions::{Hub, SessionGuard};
use crate::uri::{self, ResourceRef};

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
    fn audit(&self) -> (McpOutcome, Option<String>) {
        match self {
            Self::Denied(denial) => (McpOutcome::Denied, Some(denial.to_string())),
            Self::Api(err) => (McpOutcome::Error, Some(err.to_string())),
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
            Self::Api(err) => ErrorData::internal_error(err.to_string(), None),
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
}

impl<D: McpData> Shared<D> {
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
            "hidden": total - visible.len(),
            "messages": visible.iter().map(|m| summary_json(m)).collect::<Vec<_>>(),
        });
        // The summary aggregates every message; only show it when nothing
        // is hidden.
        if visible.len() == total {
            value["subject"] = json!(thread.thread.subject);
            value["participants"] = json!(thread.thread.participants);
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
            Some(id) => match self.data.draft(id).await {
                Ok(draft) => Some(json!({
                    "draft": draft,
                    "cursor": context.draft_cursor,
                })),
                Err(err) => {
                    tracing::debug!(%err, "open draft not readable");
                    None
                }
            },
            None => None,
        };
        self.data.emit(Event::McpContextAccess {
            client_id: client.id.clone(),
            client_name: client.name.clone(),
            edit: false,
        });
        Ok(json!({
            "account_id": context.account_id,
            "folder_id": context.folder_id,
            "thread_id": context.thread_id,
            "message": message,
            "selected_message_ids": selected,
            "draft": draft,
        }))
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
threads and rules through the nail:// resources or the read tools. Access is limited to what the \
user granted this client in Nail (Settings → MCP clients); a denied call names the missing grant. \
Every call is written to an audit log the user can see.";

/// The rmcp handler of one session.
#[derive(Clone)]
pub struct NailMcp<D: McpData> {
    shared: Arc<Shared<D>>,
    source: ClientSource,
    session: Arc<SessionGuard>,
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
        let shared = Arc::new(Shared {
            store,
            data,
            hub: Arc::new(Hub::default()),
        });
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

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(
            uri::TEMPLATES
                .iter()
                .map(|(template, name, description)| {
                    ResourceTemplate::new(*template, *name).with_description(*description)
                })
                .collect(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let client = self.client(&context.extensions).await?;
        let result = match ResourceRef::parse(&request.uri) {
            Some(resource) => {
                let access = self.shared.access(&client).await;
                self.shared
                    .read(&access, &client, &resource, &request.uri)
                    .await
            }
            None => Err(Failure::Api(ApiError::not_found(format!(
                "unknown resource {}",
                request.uri
            )))),
        };
        self.shared
            .audited(&client, McpCallKind::Resource, &request.uri, result)
            .await
            .map(ReadResourceResponse::from)
            .map_err(|f| f.to_error())
    }

    #[allow(deprecated)]
    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let client = self.client(&context.extensions).await?;
        let result = match ResourceRef::parse(&request.uri) {
            Some(resource) => {
                let access = self.shared.access(&client).await;
                self.shared
                    .may_subscribe(&access, &resource)
                    .await
                    .map(|()| resource)
            }
            None => Err(Failure::Api(ApiError::not_found(format!(
                "unknown resource {}",
                request.uri
            )))),
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

    #[allow(deprecated)]
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
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(crate::tools::definitions()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let client = self.client(&context.extensions).await?;
        crate::tools::call(&self.shared, &client, request)
            .await
            .map(CallToolResponse::from)
    }
}
