//! [`GraphEngine`]: Microsoft Graph as an
//! [`nail_api::exchange::engine::ExchangeEngine`] (ADR 0072).
//!
//! The engine holds one [`GraphClient`] per account and small caches (the
//! well-known folder ids per mailbox, the signed-in address). The work is
//! in the modules: `mail` (folders, deltas, MIME, writes, sending),
//! `settings` (categories, automatic replies, inbox rules), `directory`
//! (rooms, other mailboxes, free/busy, meeting times) and `pim`
//! (calendars, contacts, To Do). The trait implementations here only
//! convert errors.

use std::collections::HashMap;
use std::sync::Arc;

use nail_api::engine::{
    AccessTokenProvider, BoxFuture, EngineError, Envelope, FreeBusyProvider, OutgoingTransport,
    SendProgress, SendReceipt,
};
use nail_api::exchange::engine::{
    ExchangeDirectoryEngine, ExchangeEngine, ExchangeServerInfo, ExchangeSettingsEngine,
    RemoteCategory,
};
use nail_api::exchange::rules::ExchangeInboxRule;
use nail_api::exchange::{
    AutomaticRepliesSettings, ExchangeCategorySettings, ExchangeEngine as EngineKind,
    ExchangeWellKnownFolder, Room, RoomList,
};
use nail_api::scheduling::{AttendeeFreeBusy, FreeBusyQuery, InvitationResponse};
use tokio::sync::Mutex;

use crate::client::{GraphAuth, GraphClient};
use crate::error::GraphError;

/// Tunables of a [`GraphEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineOptions {
    /// Items per page (`Prefer: odata.maxpagesize`).
    pub page_size: u32,
    /// Pages read per delta call before it answers with `more`.
    pub pages_per_call: u32,
    /// How far back calendar deltas reach (days before now).
    pub calendar_past_days: u32,
    /// How far ahead calendar deltas reach (days after now).
    pub calendar_future_days: u32,
    /// Bytes per upload-session request (M4c; rounded down to a multiple
    /// of 320 KiB as Graph requires, at least 320 KiB).
    pub upload_chunk: usize,
}

impl Default for EngineOptions {
    fn default() -> Self {
        // A year back and two ahead.
        let year = u32::from(nail_util::time::DAYS_PER_YEAR);
        Self {
            page_size: 100,
            pages_per_call: 5,
            calendar_past_days: year,
            calendar_future_days: 2 * year,
            upload_chunk: crate::upload::DEFAULT_UPLOAD_CHUNK,
        }
    }
}

/// The well-known folders of one mailbox (id → name, name → id).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WellKnown {
    pub by_id: HashMap<String, ExchangeWellKnownFolder>,
}

impl WellKnown {
    pub(crate) fn id_of(&self, folder: ExchangeWellKnownFolder) -> Option<&str> {
        self.by_id
            .iter()
            .find(|(_, f)| **f == folder)
            .map(|(id, _)| id.as_str())
    }
}

/// The Graph engine of one Exchange Online account.
#[derive(Debug)]
pub struct GraphEngine {
    pub(crate) client: GraphClient,
    pub(crate) options: EngineOptions,
    /// Well-known folders per mailbox (`None` = own).
    pub(crate) well_known: Mutex<HashMap<Option<String>, WellKnown>>,
    /// The account (free/busy answers name it as their source).
    pub(crate) account_id: nail_api::AccountId,
    /// The signed-in mailbox's address (lowercase), once known: decides
    /// whether a message is sent as the user, as another mailbox or on its
    /// behalf (M4c).
    pub(crate) own_address: std::sync::Mutex<Option<String>>,
}

impl GraphEngine {
    /// Records every request into `tap` (the connection log, ADR 0190).
    #[must_use]
    pub fn with_tap(mut self, tap: nail_connlog::HttpTap) -> Self {
        self.client = self.client.with_tap(tap);
        self
    }

    /// An engine on `client` with default options.
    pub fn new(client: GraphClient) -> Self {
        Self {
            client,
            options: EngineOptions::default(),
            well_known: Mutex::new(HashMap::new()),
            account_id: nail_api::AccountId::new(""),
            own_address: std::sync::Mutex::new(None),
        }
    }

    /// Sets the signed-in mailbox's address (else the first send asks
    /// `GET /me`).
    #[must_use]
    pub fn with_own_address(self, address: &str) -> Self {
        let address = address.trim().to_ascii_lowercase();
        *self
            .own_address
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            (!address.is_empty()).then_some(address);
        self
    }

    /// The signed-in mailbox's address (cached; `GET /me` the first time
    /// when not configured).
    pub async fn own_address(&self) -> Result<String, GraphError> {
        if let Some(known) = self
            .own_address
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            return Ok(known);
        }
        let user = self.client.me(None).await?;
        let address = user
            .mail
            .or(user.user_principal_name)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if address.is_empty() {
            return Err(GraphError::Protocol("GET /me named no address".into()));
        }
        *self
            .own_address
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(address.clone());
        Ok(address)
    }

    /// Sets the account the engine belongs to.
    #[must_use]
    pub fn with_account_id(mut self, account_id: nail_api::AccountId) -> Self {
        self.account_id = account_id;
        self
    }

    /// The account the engine belongs to.
    pub fn account_id(&self) -> nail_api::AccountId {
        self.account_id.clone()
    }

    /// The engine of a configured `exchange` account with engine `graph`:
    /// the API base from its endpoint, OAuth2 tokens from `tokens` with
    /// the account's refresh-token reference.
    pub fn for_account(
        account: &nail_api::AccountConfig,
        tokens: Arc<dyn AccessTokenProvider>,
    ) -> Result<Self, GraphError> {
        use nail_api::accounts::{AuthConfig, IncomingServer};
        let IncomingServer::Exchange { server, options } = &account.incoming else {
            return Err(GraphError::Config(account.id.to_string()));
        };
        if options.engine != EngineKind::Graph {
            return Err(GraphError::Config(account.id.to_string()));
        }
        let AuthConfig::OAuth2 { provider, .. } = &server.auth else {
            return Err(GraphError::Config(account.id.to_string()));
        };
        let secret = server
            .auth
            .secret_ref(&account.id)
            .ok_or_else(|| GraphError::Config(account.id.to_string()))?;
        let auth = GraphAuth::OAuth2 {
            tokens,
            provider: provider.clone(),
            secret,
        };
        Ok(
            Self::new(GraphClient::new(&options.graph_url(server), auth)?)
                .with_account_id(account.id.clone())
                .with_own_address(&account.email),
        )
    }

    /// Replaces the options.
    #[must_use]
    pub fn with_options(mut self, options: EngineOptions) -> Self {
        self.options = options;
        self
    }

    /// The API client.
    pub fn client(&self) -> &GraphClient {
        &self.client
    }

    /// The options.
    pub fn options(&self) -> EngineOptions {
        self.options
    }

    /// The `Prefer` header asking for pages of the configured size.
    pub(crate) fn page_header(&self) -> (&'static str, String) {
        (
            "Prefer",
            format!("odata.maxpagesize={}", self.options.page_size.max(1)),
        )
    }
}

fn engine<T>(result: Result<T, GraphError>) -> Result<T, EngineError> {
    result.map_err(EngineError::from)
}

nail_exchange::exchange_mail_engine!(
    GraphEngine {
        folder_hierarchy: hierarchy,
        folder_items: messages_delta,
        fetch_mime: mime,
        write: replay,
        classify: classify_messages,
        fetch_headers: fetch_headers,
        move_to_mailbox: move_to_mailbox,
    }

    fn server_info(&self) -> BoxFuture<'_, Result<ExchangeServerInfo, EngineError>> {
        Box::pin(async move { engine(self.server_info_graph().await) })
    }

    fn folder_write<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        write: &'a nail_api::exchange::engine::FolderWrite,
    ) -> BoxFuture<'a, Result<Option<String>, EngineError>> {
        Box::pin(async move { engine(GraphEngine::folder_write(self, mailbox, write).await) })
    }

    fn wait_for_changes<'a>(
        &'a self,
        _mailbox: Option<&'a str>,
        _folders: &'a [String],
        _timeout_secs: u32,
    ) -> BoxFuture<'a, Result<Option<Vec<String>>, EngineError>> {
        // Graph pushes only to webhooks: the core polls (PollTimer).
        Box::pin(async { Ok(None) })
    }
);

impl ExchangeSettingsEngine for GraphEngine {
    fn categories(&self) -> BoxFuture<'_, Result<Vec<RemoteCategory>, EngineError>> {
        Box::pin(async move { engine(self.master_categories().await) })
    }

    fn create_category<'a>(
        &'a self,
        settings: &'a ExchangeCategorySettings,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move { engine(self.add_category(settings).await) })
    }

    fn update_category<'a>(
        &'a self,
        id: &'a str,
        settings: &'a ExchangeCategorySettings,
    ) -> BoxFuture<'a, Result<Option<String>, EngineError>> {
        Box::pin(async move { engine(self.change_category(id, settings).await) })
    }

    fn delete_category<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move { engine(self.remove_category(id).await) })
    }

    fn automatic_replies(&self) -> BoxFuture<'_, Result<AutomaticRepliesSettings, EngineError>> {
        Box::pin(async move { engine(self.read_automatic_replies(None).await) })
    }

    fn set_automatic_replies<'a>(
        &'a self,
        settings: &'a AutomaticRepliesSettings,
    ) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move { engine(self.write_automatic_replies(None, settings).await) })
    }

    fn inbox_rules(&self) -> BoxFuture<'_, Result<Vec<ExchangeInboxRule>, EngineError>> {
        Box::pin(async move { engine(self.list_rules().await) })
    }

    fn create_inbox_rule<'a>(
        &'a self,
        rule: &'a ExchangeInboxRule,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move { engine(self.add_rule(rule).await) })
    }

    fn update_inbox_rule<'a>(
        &'a self,
        rule: &'a ExchangeInboxRule,
    ) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move { engine(self.change_rule(rule).await) })
    }

    fn delete_inbox_rule<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move { engine(self.remove_rule(id).await) })
    }
}

impl ExchangeDirectoryEngine for GraphEngine {
    fn room_lists(&self) -> BoxFuture<'_, Result<Vec<RoomList>, EngineError>> {
        Box::pin(async move { engine(self.list_room_lists().await) })
    }

    fn rooms<'a>(
        &'a self,
        room_list: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<Room>, EngineError>> {
        Box::pin(async move { engine(self.list_rooms(room_list).await) })
    }

    fn check_mailbox<'a>(
        &'a self,
        address: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, EngineError>> {
        Box::pin(async move { engine(self.probe_mailbox(address).await) })
    }
}

nail_exchange::exchange_pim_engine!(GraphEngine {
    pim_folders: list_pim_folders,
    pim_changes: pim_delta,
    write_pim: replay_pim,
    respond_to_meeting: answer_meeting,
});

impl GraphEngine {
    /// Answers a meeting through the meeting API (no iMIP).
    async fn answer_meeting(
        &self,
        mailbox: Option<&str>,
        event_remote_id: &str,
        response: InvitationResponse,
        comment: Option<&str>,
        send: bool,
    ) -> Result<(), GraphError> {
        self.respond(mailbox, event_remote_id, response, comment, send, None)
            .await
    }
}

impl FreeBusyProvider for GraphEngine {
    fn free_busy<'a>(
        &'a self,
        query: &'a FreeBusyQuery,
    ) -> BoxFuture<'a, Result<Vec<AttendeeFreeBusy>, EngineError>> {
        Box::pin(async move { engine(self.schedules(query).await) })
    }
}

impl OutgoingTransport for GraphEngine {
    fn send<'a>(
        &'a self,
        envelope: &'a Envelope,
        message: &'a [u8],
    ) -> BoxFuture<'a, Result<SendReceipt, EngineError>> {
        Box::pin(async move { engine(self.send_message(envelope, message, None).await) })
    }

    fn send_with_progress<'a>(
        &'a self,
        envelope: &'a Envelope,
        message: &'a [u8],
        progress: SendProgress<'a>,
    ) -> BoxFuture<'a, Result<SendReceipt, EngineError>> {
        Box::pin(async move { engine(self.send_message(envelope, message, Some(progress)).await) })
    }
}

impl ExchangeEngine for GraphEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Graph
    }
}
