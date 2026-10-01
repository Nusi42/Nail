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
    AccessTokenProvider, BoxFuture, DeltaOutcome, EngineError, Envelope, FreeBusyProvider,
    OutgoingTransport, RemoteWrite, SendReceipt, WriteOutcome,
};
use nail_api::exchange::engine::{
    ExchangeDirectoryEngine, ExchangeEngine, ExchangeMailEngine, ExchangePimEngine,
    ExchangePimWrite, ExchangePimWriteOutcome, ExchangeServerInfo, ExchangeSettingsEngine,
    HierarchyOutcome, PimDeltaOutcome, RemoteCategory,
};
use nail_api::exchange::pim::ExchangePimFolder;
use nail_api::exchange::rules::ExchangeInboxRule;
use nail_api::exchange::{
    AutomaticRepliesSettings, ExchangeCategorySettings, ExchangeEngine as EngineKind,
    ExchangeWellKnownFolder, InferenceClassification, Room, RoomList,
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
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            page_size: 100,
            pages_per_call: 5,
            calendar_past_days: 365,
            calendar_future_days: 730,
        }
    }
}

/// The well-known folders of one mailbox (id → name, name → id).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WellKnown {
    pub by_id: HashMap<String, ExchangeWellKnownFolder>,
}

impl WellKnown {
    pub fn id_of(&self, folder: ExchangeWellKnownFolder) -> Option<&str> {
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
}

impl GraphEngine {
    /// An engine on `client` with default options.
    pub fn new(client: GraphClient) -> Self {
        Self {
            client,
            options: EngineOptions::default(),
            well_known: Mutex::new(HashMap::new()),
            account_id: nail_api::AccountId::new(""),
        }
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
                .with_account_id(account.id.clone()),
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

impl ExchangeMailEngine for GraphEngine {
    fn server_info(&self) -> BoxFuture<'_, Result<ExchangeServerInfo, EngineError>> {
        Box::pin(async move { engine(self.server_info_graph().await) })
    }

    fn folder_hierarchy<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<HierarchyOutcome, EngineError>> {
        Box::pin(async move { engine(self.hierarchy(mailbox, cursor).await) })
    }

    fn folder_items<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        folder: &'a str,
        cursor: Option<&'a str>,
        since: Option<i64>,
    ) -> BoxFuture<'a, Result<DeltaOutcome, EngineError>> {
        Box::pin(async move { engine(self.messages_delta(mailbox, folder, cursor, since).await) })
    }

    fn fetch_mime<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        remote_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<u8>, EngineError>> {
        Box::pin(async move { engine(self.mime(mailbox, remote_id).await) })
    }

    fn write<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        write: &'a RemoteWrite,
    ) -> BoxFuture<'a, Result<WriteOutcome, EngineError>> {
        Box::pin(async move { engine(self.replay(mailbox, write).await) })
    }

    fn classify<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        remote_ids: &'a [String],
        classification: InferenceClassification,
        always: bool,
    ) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move {
            engine(
                self.classify_messages(mailbox, remote_ids, classification, always)
                    .await,
            )
        })
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
}

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

impl ExchangePimEngine for GraphEngine {
    fn pim_folders<'a>(
        &'a self,
        mailbox: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<ExchangePimFolder>, EngineError>> {
        Box::pin(async move { engine(self.list_pim_folders(mailbox).await) })
    }

    fn pim_changes<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        folder: &'a ExchangePimFolder,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<PimDeltaOutcome, EngineError>> {
        Box::pin(async move { engine(self.pim_delta(mailbox, folder, cursor).await) })
    }

    fn write_pim<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        write: &'a ExchangePimWrite,
    ) -> BoxFuture<'a, Result<ExchangePimWriteOutcome, EngineError>> {
        Box::pin(async move { engine(self.replay_pim(mailbox, write).await) })
    }

    fn respond_to_meeting<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        event_remote_id: &'a str,
        response: InvitationResponse,
        comment: Option<&'a str>,
        send: bool,
    ) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move {
            engine(
                self.respond(mailbox, event_remote_id, response, comment, send, None)
                    .await,
            )
        })
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
        Box::pin(async move { engine(self.send_mime(None, envelope, message).await) })
    }
}

impl ExchangeEngine for GraphEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Graph
    }
}
