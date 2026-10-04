//! [`McpData`] on top of the core's request handling: [`CoreData`].
//!
//! The core implements the small [`CoreAccess`] trait (its request handler,
//! its store and blob store, its event bus) and wraps itself in
//! [`CoreData`]; every read then goes through the same code paths the UI
//! uses. Raw sources come from the blob store, attachments are cut out of
//! the raw source with `nail-mime`.
//!
//! M4a (WP101): writes go through [`CoreAccess::handle_write`] (by default
//! the core's request handler, so they run through its operation queue
//! like the UI's own); [`CoreAccess::confirmations`] and
//! [`CoreAccess::render`] let the core bring its confirmation broker and
//! its locale (defaults: the server's store-backed broker, English).

use std::future::Future;
use std::sync::Arc;

use nail_api::agents::ActionAttribution;
use nail_api::compose::Draft;
use nail_api::{
    Account, AccountId, ApiError, DraftId, ErrorCode, Event, Folder, FolderId, Identity,
    MessageBody, MessageId, MessageSummary, Request, Response, SearchQuery, ThreadId, UiContext,
    VirtualAccountConfig,
};
use nail_store::{BlobStore, Store};

use crate::broker::ConfirmationBroker;
use crate::data::{AttachmentData, DataResult, McpData, MessagePage, SearchOutcome, ThreadData};

/// What [`CoreData`] needs from the core.
pub trait CoreAccess: Send + Sync + 'static {
    /// Answers a Client API request (`Service::handle`).
    fn handle(&self, request: Request) -> impl Future<Output = Response> + Send;
    /// The metadata store.
    fn store(&self) -> &Store;
    /// The blob store (raw messages).
    fn blobs(&self) -> &BlobStore;
    /// Pushes an event to the connected UIs.
    fn emit(&self, event: Event);
    /// Searches the local index only (no IMAP `SEARCH` on the servers):
    /// MCP pages over local hits and never makes the core log in to a
    /// server for a client. Defaults to `Search`, minus server hits.
    fn search_local(&self, query: SearchQuery) -> impl Future<Output = Response> + Send {
        self.handle(Request::Search { query })
    }
    /// Whether the reader treats the message as end-to-end encrypted:
    /// exactly the messages whose content the core decrypts for
    /// `GetMessage` (their [`nail_api::security::SecurityStatus`] reports
    /// encryption). The `encrypted` grant is enforced with this, so it must
    /// come from the reader itself, never from a separate heuristic.
    /// Messages whose body is not downloaded are not encrypted (there is no
    /// content to read).
    fn is_encrypted(&self, id: &MessageId) -> impl Future<Output = Result<bool, ApiError>> + Send;

    /// Runs a write of an MCP client (M4a). The default is
    /// [`Self::handle`]: the request goes through the operation queue like
    /// the UI's; the core may override it to attribute the operation to
    /// `by` (audit, undo history).
    fn handle_write(
        &self,
        by: &ActionAttribution,
        request: Request,
    ) -> impl Future<Output = Response> + Send {
        let _ = by;
        self.handle(request)
    }

    /// The core's confirmation broker; `None` = the server's store-backed
    /// one ([`crate::broker::StoreBroker`]).
    fn confirmations(&self) -> Option<Arc<dyn ConfirmationBroker>> {
        None
    }

    /// The core's shared rate limiter; `None` = the server's own.
    fn rate_limiter(&self) -> Option<Arc<crate::limits::RateLimiter>> {
        None
    }

    /// Renders a user-facing text in the core's locale (default English).
    fn render(&self, message: &nail_api::LocalizedMessage) -> String {
        nail_config::l10n::english(message)
    }

    /// Tools plug-ins expose (default none).
    fn plugin_tools(&self) -> Vec<crate::data::PluginTool> {
        Vec::new()
    }

    /// Calls a plug-in tool (default `not_found`).
    fn call_plugin_tool(
        &self,
        name: &str,
        _arguments: serde_json::Value,
        _client: nail_api::agents::McpClientRef,
    ) -> impl Future<Output = Result<serde_json::Value, ApiError>> + Send {
        let name = name.to_owned();
        async move { Err(ApiError::not_found(format!("no plug-in tool {name}"))) }
    }
}

/// A response, or its error.
fn answered(response: Response) -> DataResult<Response> {
    match response {
        Response::Error {
            code,
            message,
            localized,
        } => Err(ApiError {
            code,
            message,
            localized,
        }),
        other => Ok(other),
    }
}

/// [`McpData`] for any [`CoreAccess`].
pub struct CoreData<C>(pub C);

impl<C> std::fmt::Debug for CoreData<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreData").finish_non_exhaustive()
    }
}

fn unexpected(response: Response) -> ApiError {
    match response {
        Response::Error {
            code,
            message,
            localized,
        } => ApiError {
            code,
            message,
            localized,
        },
        other => ApiError::internal(format!(
            "unexpected response {}",
            serde_json::to_value(&other)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_owned))
                .unwrap_or_default()
        )),
    }
}

impl<C: CoreAccess> CoreData<C> {
    async fn stored_raw(&self, id: &MessageId) -> DataResult<Option<Vec<u8>>> {
        let id = id.clone();
        let stored = self.0.store().call(move |s| s.get_message(&id)).await?;
        match stored.blob_hash {
            Some(hash) => Ok(Some(self.0.blobs().get(&hash)?)),
            None => Ok(None),
        }
    }
}

impl<C: CoreAccess> McpData for CoreData<C> {
    async fn accounts(&self) -> DataResult<Vec<Account>> {
        match self.0.handle(Request::ListAccounts).await {
            Response::Accounts { accounts } => Ok(accounts),
            other => Err(unexpected(other)),
        }
    }

    async fn virtual_accounts(&self) -> DataResult<Vec<VirtualAccountConfig>> {
        match self.0.handle(Request::ListVirtualAccounts).await {
            Response::VirtualAccounts { virtual_accounts } => Ok(virtual_accounts),
            other => Err(unexpected(other)),
        }
    }

    async fn folders(&self, account: &AccountId) -> DataResult<Vec<Folder>> {
        let request = Request::ListFolders {
            account_id: account.clone(),
        };
        match self.0.handle(request).await {
            Response::Folders { folders } => Ok(folders),
            other => Err(unexpected(other)),
        }
    }

    async fn messages(
        &self,
        folder: &FolderId,
        offset: u32,
        limit: u32,
    ) -> DataResult<MessagePage> {
        let request = Request::ListMessages {
            folder_id: folder.clone(),
            offset,
            limit,
        };
        match self.0.handle(request).await {
            Response::Messages {
                offset,
                total,
                messages,
                ..
            } => Ok(MessagePage {
                offset,
                total,
                messages,
            }),
            other => Err(unexpected(other)),
        }
    }

    async fn message_summary(&self, id: &MessageId) -> DataResult<MessageSummary> {
        let id = id.clone();
        Ok(self
            .0
            .store()
            .call(move |s| s.get_message(&id))
            .await?
            .summary)
    }

    async fn message_body(&self, id: &MessageId) -> DataResult<MessageBody> {
        match self.0.handle(Request::GetMessage { id: id.clone() }).await {
            Response::Message { message } => Ok(*message),
            other => Err(unexpected(other)),
        }
    }

    async fn raw_message(&self, id: &MessageId) -> DataResult<Vec<u8>> {
        if let Some(raw) = self.stored_raw(id).await? {
            return Ok(raw);
        }
        // Only headers are known: reading the message fetches the body.
        self.message_body(id).await?;
        self.stored_raw(id).await?.ok_or_else(|| {
            ApiError::new(
                ErrorCode::Unavailable,
                "the message source is not downloaded",
            )
        })
    }

    async fn attachment(&self, id: &MessageId, index: u32) -> DataResult<AttachmentData> {
        let raw = self.raw_message(id).await?;
        let content = nail_mime::attachment_content(&raw, index)
            .ok_or_else(|| ApiError::not_found(format!("attachment {index} of {id}")))?;
        Ok(AttachmentData {
            filename: content.info.filename,
            mime: content.info.mime,
            data: content.data,
        })
    }

    async fn index_holds_decrypted(&self) -> DataResult<bool> {
        let settings = self
            .0
            .store()
            .call(|s| {
                s.setting::<nail_api::security::SecuritySettings>(nail_store::SECURITY_SETTINGS_KEY)
            })
            .await?;
        Ok(settings.is_some_and(|s| !s.index_decrypted.is_empty()))
    }

    async fn is_encrypted(&self, id: &MessageId) -> DataResult<bool> {
        self.0.is_encrypted(id).await
    }

    async fn pim(&self, request: Request) -> DataResult<Response> {
        answered(self.0.handle(request).await)
    }

    async fn query(&self, request: Request) -> DataResult<Response> {
        answered(self.0.handle(request).await)
    }

    async fn write(&self, by: &ActionAttribution, request: Request) -> DataResult<Response> {
        answered(self.0.handle_write(by, request).await)
    }

    fn confirmations(&self) -> Option<Arc<dyn ConfirmationBroker>> {
        self.0.confirmations()
    }

    fn rate_limiter(&self) -> Option<Arc<crate::limits::RateLimiter>> {
        self.0.rate_limiter()
    }

    fn render(&self, message: &nail_api::LocalizedMessage) -> String {
        self.0.render(message)
    }

    fn plugin_tools(&self) -> Vec<crate::data::PluginTool> {
        self.0.plugin_tools()
    }

    async fn call_plugin_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
        client: nail_api::agents::McpClientRef,
    ) -> DataResult<serde_json::Value> {
        self.0.call_plugin_tool(name, arguments, client).await
    }

    async fn pim_raw(&self, item: &nail_api::pim::PimItemRef) -> DataResult<Option<String>> {
        let item = item.clone();
        Ok(self
            .0
            .store()
            .call(move |s| s.pim_object(&item))
            .await?
            .map(|o| o.raw))
    }

    async fn thread(&self, id: &ThreadId) -> DataResult<ThreadData> {
        let request = Request::GetThread {
            thread_id: id.clone(),
        };
        match self.0.handle(request).await {
            Response::Thread { thread, messages } => Ok(ThreadData {
                thread: *thread,
                messages,
            }),
            other => Err(unexpected(other)),
        }
    }

    async fn search(&self, query: SearchQuery) -> DataResult<SearchOutcome> {
        match self.0.search_local(query).await {
            Response::SearchResults {
                offset,
                total,
                mut hits,
                server,
            } => {
                // Paging counts local hits only.
                hits.retain(|h| !h.from_server);
                let extra = server.map_or(0, |s| s.hits);
                Ok(SearchOutcome::Hits {
                    offset,
                    total: total.saturating_sub(extra),
                    hits,
                })
            }
            Response::InvalidQuery { error } => Ok(SearchOutcome::Invalid(error)),
            other => Err(unexpected(other)),
        }
    }

    async fn rules_text(&self) -> DataResult<String> {
        let rules = match self.0.handle(Request::ListRules).await {
            Response::Rules { rules } => rules,
            other => return Err(unexpected(other)),
        };
        let mut text = String::new();
        for rule in rules {
            match self
                .0
                .handle(Request::RuleToText {
                    rule: Box::new(rule),
                })
                .await
            {
                Response::RuleText { text: one } => {
                    text.push_str(one.trim_end());
                    text.push('\n');
                }
                other => return Err(unexpected(other)),
            }
        }
        Ok(text)
    }

    async fn ui_context(&self) -> DataResult<UiContext> {
        match self.0.handle(Request::GetUiContext).await {
            Response::UiContext { context } => Ok(context),
            other => Err(unexpected(other)),
        }
    }

    async fn identities(&self) -> DataResult<Vec<Identity>> {
        match self
            .0
            .handle(Request::ListIdentities { account_id: None })
            .await
        {
            Response::Identities { identities } => Ok(identities),
            other => Err(unexpected(other)),
        }
    }

    async fn draft(&self, id: &DraftId) -> DataResult<Draft> {
        let request = Request::GetDraft {
            draft_id: id.clone(),
        };
        match self.0.handle(request).await {
            Response::Draft { draft } => Ok(*draft),
            other => Err(unexpected(other)),
        }
    }

    fn emit(&self, event: Event) {
        self.0.emit(event);
    }
}
