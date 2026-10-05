//! Exchange accounts (M3c: contract ADR 0072, integration ADR 0077).
//!
//! The Graph engine (`nail-proto-graph`, ADR 0073) and the EWS engine
//! (`nail-proto-ews`, ADR 0074) — both a
//! [`nail_api::exchange::engine::ExchangeEngine`] — and the common layer
//! (`nail-exchange`, ADR 0075: Autodiscover, the SSPI `NegotiateAuth`, the
//! mapping between Exchange and Nail models) are wired here:
//!
//! * [`sync`] — the account engine: mail per mailbox (hierarchy, then
//!   per-folder deltas with a cursor each), replay of the mail operation
//!   queue (`RemoteWrite`s by remote id; a move stores the new id) and of
//!   the Exchange operation queue, Graph polling with [`PollTimer`] /
//!   [`FolderPoll`] or EWS streaming notifications.
//! * [`settings`] — the master category list, automatic replies, inbox
//!   rules and the placement of Nail rules as inbox rules.
//! * [`pim`] — the account's `exchange:<account>` PIM source: calendars,
//!   contact folders and task lists of the own mailbox and the shared /
//!   delegate ones, PIM queue replay (meetings answered through the
//!   engine's meeting API instead of iMIP).
//! * [`setup`] — engines for an account, `TestAccount`, Autodiscover for
//!   `DiscoverAccount`.
//! * This module — the 18 Exchange requests.
//!
//! [`PollTimer`]: nail_proto_graph::PollTimer
//! [`FolderPoll`]: nail_proto_graph::FolderPoll

pub(crate) mod batch;
pub(crate) mod pim;
pub(crate) mod settings;
pub(crate) mod setup;
pub(crate) mod sync;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nail_api::accounts::TlsMode;
use nail_api::exchange::engine::{ExchangeEngine, NegotiateAuth};
use nail_api::exchange::{
    AutomaticReplies, ExchangeArea, ExchangeCategory, ExchangeCategorySettings,
    ExchangeMailboxIssue, ExchangeMailboxOrigin, ExchangeMailboxSettings, ExchangeOptions,
    ExchangePush, ExchangeStatus, ExchangeWarning, RoomAvailability,
};
use nail_api::scheduling::FreeBusyQuery;
use nail_api::{
    AccountConfig, AccountId, ApiError, ErrorCode, Event, IncomingServer, LocalizedMessage,
    MessageId, OpId, Request, Response, ServerEndpoint,
};
use nail_store::exchange::ExchangeOp;
use nail_store::{MessageFilter, Operation};

use crate::engine::unix_now;
use crate::error::{CoreError, Result};
use crate::service::{Service, lock};

pub(crate) use sync::run;

/// The Microsoft Graph endpoint Graph proposals and accounts point at
/// instead of `graph.microsoft.com` (seeded demos: the mock Graph server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEndpoint {
    /// Host.
    pub host: String,
    /// Port.
    pub port: u16,
    /// TLS (`plaintext` only for loopback hosts).
    pub tls: TlsMode,
    /// The OAuth2 provider id to sign in with.
    pub provider: String,
}

/// How Exchange accounts reach the outside world
/// ([`crate::ServiceOptions::exchange`]).
#[derive(Clone, Default)]
pub struct ExchangeHooks {
    /// Autodiscover's HTTP; `None` = the real network (nail-auth's TLS).
    pub autodiscover_http: Option<Arc<dyn nail_exchange::http::ExchangeHttp>>,
    /// SRV lookups of Autodiscover; `None` = the OS resolver.
    pub autodiscover_dns: Option<Arc<dyn nail_discovery::DnsLookup>>,
    /// Do not ask Microsoft's global Autodiscover endpoint (it receives the
    /// address).
    pub no_microsoft_lookup: bool,
    /// Windows sign-in for EWS accounts and Autodiscover instead of SSPI
    /// (tests: a scripted handshake).
    pub negotiate: Option<Arc<dyn NegotiateAuth>>,
    /// Where Graph proposals point (demos); `None` = Microsoft Graph.
    pub graph_endpoint: Option<GraphEndpoint>,
}

impl std::fmt::Debug for ExchangeHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExchangeHooks")
            .field("autodiscover_http", &self.autodiscover_http.is_some())
            .field("autodiscover_dns", &self.autodiscover_dns.is_some())
            .field("no_microsoft_lookup", &self.no_microsoft_lookup)
            .field("negotiate", &self.negotiate.is_some())
            .field("graph_endpoint", &self.graph_endpoint)
            .finish()
    }
}

/// A running account engine as the requests see it.
#[derive(Debug)]
pub(crate) struct RunningEngine {
    /// The engine.
    pub engine: Arc<dyn ExchangeEngine>,
    /// Its grouped mail writes (P3); `None` = one write at a time.
    pub batch: Option<Arc<dyn batch::BatchWrites>>,
    /// How changes arrive now.
    pub push: Mutex<ExchangePush>,
    /// Warnings found while syncing (Focused Inbox unavailable, rooms need
    /// consent).
    pub warnings: Mutex<Vec<ExchangeWarning>>,
    /// Wakes the engine for a settings pass (rules, categories, automatic
    /// replies).
    pub settings_wake: tokio::sync::Notify,
}

/// The Exchange state of the core.
#[derive(Debug)]
pub(crate) struct ExchangeCtl {
    pub hooks: ExchangeHooks,
    engines: Mutex<HashMap<AccountId, Arc<RunningEngine>>>,
    /// Automapped mailboxes Autodiscover found, by address (lowercase),
    /// stored when the account is added.
    discovered: Mutex<HashMap<String, Vec<ExchangeMailboxSettings>>>,
    /// Hosts whose sign-in prompt Autodiscover showed, by address
    /// (lowercase): a Windows sign-in goes only there (ADR 0078).
    login_prompts: Mutex<HashMap<String, Vec<String>>>,
    /// Warnings of accounts without a running engine (rooms asked through
    /// a temporary engine).
    warnings: Mutex<HashMap<AccountId, Vec<ExchangeWarning>>>,
}

impl ExchangeCtl {
    pub(crate) fn new(hooks: ExchangeHooks) -> Self {
        Self {
            hooks,
            engines: Mutex::new(HashMap::new()),
            discovered: Mutex::new(HashMap::new()),
            login_prompts: Mutex::new(HashMap::new()),
            warnings: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn register(&self, account: &AccountId, running: Arc<RunningEngine>) {
        lock(&self.engines).insert(account.clone(), running);
    }

    pub(crate) fn unregister(&self, account: &AccountId, running: &Arc<RunningEngine>) {
        let mut engines = lock(&self.engines);
        if engines
            .get(account)
            .is_some_and(|r| Arc::ptr_eq(r, running))
        {
            engines.remove(account);
        }
    }

    pub(crate) fn running(&self, account: &AccountId) -> Option<Arc<RunningEngine>> {
        lock(&self.engines).get(account).cloned()
    }

    /// Every running engine (free/busy asks them all).
    pub(crate) fn all_running(&self) -> Vec<(AccountId, Arc<RunningEngine>)> {
        let mut out: Vec<_> = lock(&self.engines)
            .iter()
            .map(|(a, r)| (a.clone(), Arc::clone(r)))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Remembers the host a Windows sign-in prompt named for an address
    /// (the next run may sign in there, and only there).
    pub(crate) fn remember_login_prompt(&self, email: &str, host: &str) {
        let mut prompts = lock(&self.login_prompts);
        let hosts = prompts
            .entry(email.trim().to_ascii_lowercase())
            .or_default();
        let host = host.trim().to_ascii_lowercase();
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }

    /// The hosts an address's Windows sign-in may go to.
    pub(crate) fn login_hosts(&self, email: &str) -> Vec<String> {
        lock(&self.login_prompts)
            .get(&email.trim().to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn remember_discovery(&self, email: &str, mailboxes: Vec<ExchangeMailboxSettings>) {
        lock(&self.discovered).insert(email.trim().to_ascii_lowercase(), mailboxes);
    }

    pub(crate) fn take_discovery(&self, email: &str) -> Vec<ExchangeMailboxSettings> {
        lock(&self.discovered)
            .remove(&email.trim().to_ascii_lowercase())
            .unwrap_or_default()
    }

    fn add_warning(&self, account: &AccountId, warning: ExchangeWarning) {
        if let Some(running) = self.running(account) {
            let mut list = lock(&running.warnings);
            if !list.contains(&warning) {
                list.push(warning);
            }
            return;
        }
        let mut all = lock(&self.warnings);
        let list = all.entry(account.clone()).or_default();
        if !list.contains(&warning) {
            list.push(warning);
        }
    }

    fn warnings_of(&self, account: &AccountId) -> Vec<ExchangeWarning> {
        let mut out = self
            .running(account)
            .map(|r| lock(&r.warnings).clone())
            .unwrap_or_default();
        for w in lock(&self.warnings)
            .get(account)
            .cloned()
            .unwrap_or_default()
        {
            if !out.contains(&w) {
                out.push(w);
            }
        }
        out
    }
}

/// The configuration of an Exchange account (`invalid_request` for
/// others).
pub(crate) fn exchange_account(service: &Service, id: &AccountId) -> Result<AccountConfig> {
    let config = service.inner.accounts.config();
    let account = config
        .account(id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("account {id}")))?;
    if !matches!(account.incoming, IncomingServer::Exchange { .. }) {
        return Err(
            ApiError::invalid_request(format!("account {id} is not an Exchange account")).into(),
        );
    }
    Ok(account)
}

/// The Exchange server and options of an account.
pub(crate) fn exchange_parts(
    account: &AccountConfig,
) -> Option<(&ServerEndpoint, &ExchangeOptions)> {
    match &account.incoming {
        IncomingServer::Exchange { server, options } => Some((server, options)),
        _ => None,
    }
}

/// The engine of an account: the running one, else a new one (disabled
/// accounts, the room finder before the first sync).
pub(crate) fn engine_of(
    service: &Service,
    account: &AccountConfig,
) -> Result<Arc<dyn ExchangeEngine>> {
    if let Some(running) = service.inner.exchange.running(&account.id) {
        return Ok(Arc::clone(&running.engine));
    }
    setup::engine_for(service, account)
}

fn rfc3339(secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Tells clients that areas of an account changed.
pub(crate) fn changed(service: &Service, account: &AccountId, areas: &[ExchangeArea]) {
    service.broadcast(Event::ExchangeChanged {
        account_id: account.clone(),
        areas: areas.to_vec(),
    });
}

/// `invalid_request` with the first issue's text.
fn invalid<T: Copy>(what: &str, issues: &[T], id: impl Fn(T) -> &'static str) -> CoreError {
    let ids: Vec<&str> = issues.iter().map(|i| id(*i)).collect();
    let mut err = ApiError::invalid_request(format!("invalid {what}: {}", ids.join(", ")));
    if let Some(first) = issues.first() {
        err = err.with_localized(LocalizedMessage::new(id(*first)));
    }
    err.into()
}

fn store_invalid(err: nail_store::Error) -> CoreError {
    match err {
        nail_store::Error::InvalidData(message) => ApiError::invalid_request(message).into(),
        other => other.into(),
    }
}

/// Queues an Exchange operation (applied locally at once) and wakes the
/// account's engine.
async fn queue(
    service: &Service,
    account: &AccountId,
    op: ExchangeOp,
) -> Result<(Option<String>, OpId)> {
    let id = account.clone();
    let queued = service
        .store()
        .call(move |s| s.enqueue_exchange_op(&id, &op))
        .await
        .map_err(store_invalid)?;
    service.inner.engines.request_replay(account);
    Ok(queued)
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetExchangeStatus { account_id } => Ok(Response::ExchangeStatus {
            status: Box::new(status(service, &account_id).await?),
        }),
        Request::ListExchangeMailboxes { account_id } => {
            exchange_account(service, &account_id)?;
            let id = account_id.clone();
            let mailboxes = service
                .store()
                .call(move |s| s.exchange_mailboxes(&id, false))
                .await?;
            Ok(Response::ExchangeMailboxes {
                account_id,
                mailboxes,
            })
        }
        Request::AddExchangeMailbox {
            account_id,
            settings,
        } => save_mailbox(service, &account_id, settings, true).await,
        Request::UpdateExchangeMailbox {
            account_id,
            settings,
        } => save_mailbox(service, &account_id, settings, false).await,
        Request::RemoveExchangeMailbox {
            account_id,
            address,
        } => {
            exchange_account(service, &account_id)?;
            let (id, key) = (account_id.clone(), address.clone());
            let removed = service
                .store()
                .call(move |s| s.remove_exchange_mailbox(&id, &key))
                .await?;
            if !removed {
                return Err(ApiError::not_found(format!("mailbox {address}")).into());
            }
            sync::forget_mailbox(service, &account_id, &address).await?;
            pim::forget_mailbox(service, &account_id, &address).await?;
            changed(service, &account_id, &[ExchangeArea::Mailboxes]);
            Ok(Response::Done)
        }
        Request::ListExchangeCategories { account_id } => {
            exchange_account(service, &account_id)?;
            let id = account_id.clone();
            let categories = service
                .store()
                .call(move |s| s.list_exchange_categories(&id))
                .await?;
            Ok(Response::ExchangeCategories {
                account_id,
                categories,
            })
        }
        Request::CreateExchangeCategory {
            account_id,
            settings,
        } => {
            exchange_account(service, &account_id)?;
            check_category(service, &account_id, &settings, None).await?;
            let (local, op_id) = queue(
                service,
                &account_id,
                ExchangeOp::CreateCategory { settings },
            )
            .await?;
            let local = local.ok_or_else(|| ApiError::internal("no category id"))?;
            changed(service, &account_id, &[ExchangeArea::Categories]);
            Ok(Response::ExchangeCategorySaved {
                category: Box::new(category(service, &account_id, &local).await?),
                op_ids: vec![op_id],
            })
        }
        Request::UpdateExchangeCategory {
            account_id,
            category_id,
            settings,
        } => {
            exchange_account(service, &account_id)?;
            let before = category(service, &account_id, category_id.as_str()).await?;
            check_category(service, &account_id, &settings, Some(category_id.as_str())).await?;
            let (_, op_id) = queue(
                service,
                &account_id,
                ExchangeOp::UpdateCategory {
                    category_id: category_id.clone(),
                    settings: settings.clone(),
                },
            )
            .await?;
            let mut op_ids = vec![op_id];
            if before.name.trim() != settings.name.trim() {
                // The tag follows the category, through the mail queue.
                op_ids
                    .extend(retag(service, &account_id, &before.name, settings.name.trim()).await?);
            }
            changed(service, &account_id, &[ExchangeArea::Categories]);
            Ok(Response::ExchangeCategorySaved {
                category: Box::new(category(service, &account_id, category_id.as_str()).await?),
                op_ids,
            })
        }
        Request::DeleteExchangeCategory {
            account_id,
            category_id,
        } => {
            exchange_account(service, &account_id)?;
            category(service, &account_id, category_id.as_str()).await?;
            let (_, op_id) = queue(
                service,
                &account_id,
                ExchangeOp::DeleteCategory { category_id },
            )
            .await?;
            changed(service, &account_id, &[ExchangeArea::Categories]);
            Ok(Response::Queued {
                op_ids: vec![op_id],
            })
        }
        Request::ClassifyFocused {
            message_ids,
            classification,
            always,
        } => classify(service, message_ids, classification, always).await,
        Request::GetAutomaticReplies { account_id } => {
            exchange_account(service, &account_id)?;
            Ok(Response::AutomaticReplies {
                replies: Box::new(replies(service, &account_id).await?),
                op_ids: Vec::new(),
            })
        }
        Request::SetAutomaticReplies {
            account_id,
            settings,
        } => {
            exchange_account(service, &account_id)?;
            let issues = settings.issues();
            if !issues.is_empty() {
                return Err(invalid(
                    "automatic replies",
                    &issues,
                    nail_api::exchange::oof::AutomaticRepliesIssue::message_id,
                ));
            }
            let (_, op_id) = queue(
                service,
                &account_id,
                ExchangeOp::SetAutomaticReplies { settings },
            )
            .await?;
            changed(service, &account_id, &[ExchangeArea::AutomaticReplies]);
            Ok(Response::AutomaticReplies {
                replies: Box::new(replies(service, &account_id).await?),
                op_ids: vec![op_id],
            })
        }
        Request::GetExchangeRules { account_id } => {
            let account = exchange_account(service, &account_id)?;
            Ok(Response::ExchangeRules {
                status: Box::new(settings::rules_status(service, &account).await?),
            })
        }
        Request::SyncExchangeRules { account_id } => {
            let account = exchange_account(service, &account_id)?;
            settings::request_rules_sync(service, &account).await?;
            Ok(Response::ExchangeRules {
                status: Box::new(settings::rules_status(service, &account).await?),
            })
        }
        Request::SetExchangeInboxRuleEnabled {
            account_id,
            server_rule_id,
            enabled,
        } => {
            exchange_account(service, &account_id)?;
            settings::server_rule(service, &account_id, &server_rule_id).await?;
            let (_, op_id) = queue(
                service,
                &account_id,
                ExchangeOp::SetRuleEnabled {
                    server_rule_id,
                    enabled,
                },
            )
            .await?;
            changed(service, &account_id, &[ExchangeArea::Rules]);
            Ok(Response::Queued {
                op_ids: vec![op_id],
            })
        }
        Request::DeleteExchangeInboxRule {
            account_id,
            server_rule_id,
        } => {
            exchange_account(service, &account_id)?;
            settings::server_rule(service, &account_id, &server_rule_id).await?;
            let (_, op_id) = queue(
                service,
                &account_id,
                ExchangeOp::DeleteRule { server_rule_id },
            )
            .await?;
            changed(service, &account_id, &[ExchangeArea::Rules]);
            Ok(Response::Queued {
                op_ids: vec![op_id],
            })
        }
        Request::ListRoomLists {
            account_id,
            refresh,
        } => {
            let account = exchange_account(service, &account_id)?;
            let lists = room_lists(service, &account, refresh).await?;
            Ok(Response::RoomLists { account_id, lists })
        }
        Request::FindRooms {
            account_id,
            room_list,
            from,
            to,
            min_capacity,
            refresh,
        } => {
            let account = exchange_account(service, &account_id)?;
            let rooms = find_rooms(
                service,
                &account,
                room_list.as_deref(),
                &from,
                &to,
                min_capacity,
                refresh,
            )
            .await?;
            Ok(Response::Rooms {
                account_id,
                from,
                to,
                rooms,
            })
        }
        other => Err(ApiError::internal(format!(
            "{} routed to Exchange",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

/// `GetExchangeStatus`.
async fn status(service: &Service, account_id: &AccountId) -> Result<ExchangeStatus> {
    let account = exchange_account(service, account_id)?;
    let Some((server, options)) = exchange_parts(&account) else {
        return Err(ApiError::invalid_request(format!(
            "account {account_id} is not an Exchange account"
        ))
        .into());
    };
    let id = account_id.clone();
    let (record, folders, pending) = service
        .store()
        .call(move |s| {
            Ok((
                s.exchange_sync(&id)?,
                s.exchange_folders(&id)?,
                s.pending_exchange_op_count(&id)?,
            ))
        })
        .await?;
    let now = unix_now();
    let throttled = record
        .as_ref()
        .and_then(|r| r.throttled_until)
        .filter(|t| *t > now);
    let mut warnings = options.warnings(server);
    for w in service.inner.exchange.warnings_of(account_id) {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    }
    if throttled.is_some() && !warnings.contains(&ExchangeWarning::Throttled) {
        warnings.push(ExchangeWarning::Throttled);
    }
    let push = match service.inner.exchange.running(account_id) {
        Some(running) => *lock(&running.push),
        None => ExchangePush::None,
    };
    let total = u32::try_from(folders.len()).unwrap_or(u32::MAX);
    let synced =
        u32::try_from(folders.iter().filter(|f| f.cursor.is_some()).count()).unwrap_or(u32::MAX);
    Ok(ExchangeStatus {
        account_id: account_id.clone(),
        engine: options.engine,
        server_version: record.as_ref().and_then(|r| r.server_version.clone()),
        push,
        last_sync_at: record
            .as_ref()
            .and_then(|r| r.last_sync_at)
            .and_then(rfc3339),
        throttled_until: throttled.and_then(rfc3339),
        folders_synced: synced,
        folders_total: total,
        pending_ops: pending,
        warnings,
    })
}

/// `AddExchangeMailbox` / `UpdateExchangeMailbox`.
async fn save_mailbox(
    service: &Service,
    account_id: &AccountId,
    mut settings: ExchangeMailboxSettings,
    add: bool,
) -> Result<Response> {
    let account = exchange_account(service, account_id)?;
    settings.address = settings.address.trim().to_ascii_lowercase();
    let mut issues = settings.issues(&account.email);
    let id = account_id.clone();
    let existing = service
        .store()
        .call(move |s| s.exchange_mailboxes(&id, true))
        .await?;
    let found = existing
        .iter()
        .find(|m| m.settings.address == settings.address);
    let hidden_now = {
        let id = account_id.clone();
        let visible = service
            .store()
            .call(move |s| s.exchange_mailboxes(&id, false))
            .await?;
        found.is_some_and(|m| {
            !visible
                .iter()
                .any(|v| v.settings.address == m.settings.address)
        })
    };
    if add && found.is_some() && !hidden_now && issues.is_empty() {
        issues.push(ExchangeMailboxIssue::Duplicate);
    }
    if !issues.is_empty() {
        return Err(invalid(
            "mailbox",
            &issues,
            ExchangeMailboxIssue::message_id,
        ));
    }
    if !add && found.is_none() {
        return Err(ApiError::not_found(format!("mailbox {}", settings.address)).into());
    }
    let origin = found.map_or(ExchangeMailboxOrigin::User, |m| m.origin);
    let (id, record) = (account_id.clone(), settings.clone());
    let now = unix_now();
    let mailbox = service
        .store()
        .call(move |s| s.save_exchange_mailbox(&id, &record, origin, now))
        .await?;
    if !add {
        // Parts no longer shown go now; new parts come with the next sync.
        if !settings.mail {
            sync::forget_mailbox(service, account_id, &settings.address).await?;
        }
        pim::forget_unshown(service, account_id, &settings).await?;
    }
    changed(service, account_id, &[ExchangeArea::Mailboxes]);
    service.inner.engines.request_sync(account_id);
    Ok(Response::ExchangeMailboxSaved {
        mailbox: Box::new(mailbox),
    })
}

async fn category(
    service: &Service,
    account: &AccountId,
    category_id: &str,
) -> Result<ExchangeCategory> {
    let id = account.clone();
    service
        .store()
        .call(move |s| s.list_exchange_categories(&id))
        .await?
        .into_iter()
        .find(|c| c.id.as_str() == category_id)
        .ok_or_else(|| ApiError::not_found(format!("category {category_id}")).into())
}

/// The contract's checks plus a duplicate name (`invalid_request`).
async fn check_category(
    service: &Service,
    account: &AccountId,
    settings: &ExchangeCategorySettings,
    except: Option<&str>,
) -> Result<()> {
    let id = account.clone();
    let others = service
        .store()
        .call(move |s| s.list_exchange_categories(&id))
        .await?;
    let issues = nail_exchange::mapping::categories::issues(settings, &others, except);
    if !issues.is_empty() {
        return Err(invalid(
            "category",
            &issues,
            nail_api::exchange::ExchangeCategoryIssue::message_id,
        ));
    }
    Ok(())
}

/// Renames a tag on every message of the account (mail queue).
async fn retag(service: &Service, account: &AccountId, old: &str, new: &str) -> Result<Vec<OpId>> {
    let filter =
        MessageFilter::AccountIn(vec![account.clone()]).and(MessageFilter::Tag(old.into()));
    let ids = service
        .store()
        .call(move |s| {
            let ids = s.filter_message_ids(&filter)?;
            s.message_summaries(&ids)
        })
        .await?;
    let mut ops = Vec::new();
    for summary in ids {
        let Some(tags) = nail_exchange::mapping::categories::retag(&summary.tags, old, new) else {
            continue;
        };
        let remove: Vec<String> = summary
            .tags
            .iter()
            .filter(|t| !tags.contains(t))
            .cloned()
            .collect();
        let add: Vec<String> = tags
            .iter()
            .filter(|t| !summary.tags.contains(t))
            .cloned()
            .collect();
        ops.push(Operation::UpdateFlags {
            message_id: summary.id.clone(),
            change: nail_store::FlagChange {
                add_tags: add,
                remove_tags: remove,
                ..nail_store::FlagChange::default()
            },
        });
    }
    service.enqueue(ops).await
}

/// `ClassifyFocused`: Exchange inbox messages of one account.
async fn classify(
    service: &Service,
    message_ids: Vec<MessageId>,
    classification: nail_api::exchange::InferenceClassification,
    always: bool,
) -> Result<Response> {
    if message_ids.is_empty() {
        return Err(ApiError::invalid_request("no messages").into());
    }
    let ids = message_ids.clone();
    let summaries = service
        .store()
        .call(move |s| s.message_summaries(&ids))
        .await?;
    if summaries.len() != message_ids.len() {
        return Err(ApiError::not_found("a message is gone").into());
    }
    let account = summaries[0].account_id.clone();
    if summaries.iter().any(|m| m.account_id != account) {
        return Err(ApiError::invalid_request("the messages belong to different accounts").into());
    }
    exchange_account(service, &account)?;
    if summaries.iter().any(|m| m.exchange.is_none()) {
        return Err(ApiError::invalid_request("not an Exchange message").into());
    }
    let (_, op_id) = queue(
        service,
        &account,
        ExchangeOp::Classify {
            message_ids,
            classification,
            always,
        },
    )
    .await?;
    service.broadcast(Event::MailChanged {
        folder_ids: summaries.iter().map(|m| m.folder_id.clone()).collect(),
    });
    Ok(Response::Queued {
        op_ids: vec![op_id],
    })
}

pub(crate) async fn replies(service: &Service, account: &AccountId) -> Result<AutomaticReplies> {
    let id = account.clone();
    Ok(service
        .store()
        .call(move |s| s.exchange_automatic_replies(&id))
        .await?)
}

/// The rooms' consent warning as an error.
fn rooms_forbidden(
    service: &Service,
    account: &AccountId,
    err: &nail_api::engine::EngineError,
) -> CoreError {
    service
        .inner
        .exchange
        .add_warning(account, ExchangeWarning::RoomsNeedConsent);
    changed(service, account, &[ExchangeArea::Status]);
    ApiError::new(ErrorCode::Forbidden, err.message.clone())
        .with_localized(LocalizedMessage::new(
            ExchangeWarning::RoomsNeedConsent.message_id(),
        ))
        .into()
}

/// `ListRoomLists`: the cache, or the server when asked (or never asked).
async fn room_lists(
    service: &Service,
    account: &AccountConfig,
    refresh: bool,
) -> Result<Vec<nail_api::exchange::RoomList>> {
    let id = account.id.clone();
    let (cached, fetched) = service
        .store()
        .call(move |s| s.exchange_room_lists(&id))
        .await?;
    if fetched.is_some() && !refresh {
        return Ok(cached);
    }
    let engine = engine_of(service, account)?;
    let lists = match engine.room_lists().await {
        Ok(lists) => lists,
        Err(err) if err.code == ErrorCode::Forbidden => {
            return Err(rooms_forbidden(service, &account.id, &err));
        }
        Err(err) => return Err(err.into()),
    };
    let (id, record) = (account.id.clone(), lists.clone());
    let now = unix_now();
    service
        .store()
        .call(move |s| s.replace_exchange_room_lists(&id, &record, now))
        .await?;
    let mut lists = lists;
    lists.sort_by_key(|a| a.name.to_lowercase());
    Ok(lists)
}

/// `FindRooms`: rooms (cached per room list) with their availability.
async fn find_rooms(
    service: &Service,
    account: &AccountConfig,
    room_list: Option<&str>,
    from: &str,
    to: &str,
    min_capacity: Option<u32>,
    refresh: bool,
) -> Result<Vec<RoomAvailability>> {
    // The range rules of free/busy (one stand-in attendee).
    let issues =
        nail_api::scheduling::free_busy_issues(&["room@nail.invalid".to_owned()], from, to);
    if !issues.is_empty() {
        return Err(invalid(
            "range",
            &issues,
            nail_api::scheduling::FreeBusyIssue::message_id,
        ));
    }
    let id = account.id.clone();
    let key = room_list.map(str::to_owned);
    let (cached, fetched) = service
        .store()
        .call(move |s| s.exchange_rooms(&id, key.as_deref()))
        .await?;
    let engine = engine_of(service, account)?;
    let rooms = if fetched.is_some() && !refresh {
        cached
    } else {
        let rooms = match engine.rooms(room_list).await {
            Ok(rooms) => rooms,
            Err(err) if err.code == ErrorCode::Forbidden => {
                return Err(rooms_forbidden(service, &account.id, &err));
            }
            Err(err) => return Err(err.into()),
        };
        let (id, key, record) = (
            account.id.clone(),
            room_list.map(str::to_owned),
            rooms.clone(),
        );
        let now = unix_now();
        service
            .store()
            .call(move |s| s.replace_exchange_rooms(&id, key.as_deref(), &record, now))
            .await?;
        rooms
    };
    let rooms = nail_exchange::mapping::rooms::filter_rooms(&rooms, room_list, min_capacity);
    if rooms.is_empty() {
        return Ok(Vec::new());
    }
    let query = FreeBusyQuery {
        organizer: account.email.clone(),
        attendees: rooms.iter().map(|r| r.address.clone()).collect(),
        from: from.to_owned(),
        to: to.to_owned(),
    };
    let answers = match engine.free_busy(&query).await {
        Ok(answers) => answers,
        Err(err) => {
            tracing::info!(account = %account.id, %err, "room availability failed");
            Vec::new()
        }
    };
    Ok(nail_exchange::mapping::rooms::availability(
        &rooms, &answers, from, to,
    ))
}

/// Free/busy of people and rooms through every running Exchange engine
/// (`QueryFreeBusy`; [`nail_api::scheduling::FreeBusySource::Exchange`]).
/// Answers what the engines know; attendees nobody knows are left out.
pub(crate) async fn free_busy(
    service: &Service,
    query: &FreeBusyQuery,
) -> Vec<nail_api::scheduling::AttendeeFreeBusy> {
    let mut out: Vec<nail_api::scheduling::AttendeeFreeBusy> = Vec::new();
    let mut rest = query.attendees.clone();
    for (account, running) in service.inner.exchange.all_running() {
        if rest.is_empty() {
            break;
        }
        let ask = FreeBusyQuery {
            attendees: rest.clone(),
            ..query.clone()
        };
        match running.engine.free_busy(&ask).await {
            Ok(found) => {
                for entry in found {
                    if entry.error.is_none() || !entry.periods.is_empty() {
                        rest.retain(|a| !a.eq_ignore_ascii_case(&entry.email));
                        out.push(entry);
                    }
                }
            }
            Err(err) => {
                tracing::debug!(%account, %err, "Exchange free/busy failed");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_sign_in_goes_only_to_prompted_hosts() {
        let ctl = ExchangeCtl::new(ExchangeHooks::default());
        assert!(ctl.login_hosts("ada@corp.example").is_empty());
        ctl.remember_login_prompt("Ada@Corp.example ", "Autodiscover.corp.example");
        ctl.remember_login_prompt("ada@corp.example", "autodiscover.corp.example");
        assert_eq!(
            ctl.login_hosts("ada@corp.example"),
            ["autodiscover.corp.example"]
        );
        assert!(ctl.login_hosts("grace@corp.example").is_empty());
    }

    #[test]
    fn hooks_debug_hides_details() {
        let hooks = ExchangeHooks {
            graph_endpoint: Some(GraphEndpoint {
                host: "127.0.0.1".into(),
                port: 1,
                tls: TlsMode::Plaintext,
                provider: "demo".into(),
            }),
            ..ExchangeHooks::default()
        };
        let text = format!("{hooks:?}");
        assert!(text.contains("graph_endpoint") && text.contains("127.0.0.1"));
        assert!(text.contains("negotiate: false"));
    }

    #[test]
    fn discovery_is_taken_once() {
        let ctl = ExchangeCtl::new(ExchangeHooks::default());
        ctl.remember_discovery(
            "Ada@Contoso.example",
            vec![ExchangeMailboxSettings::shared("support@contoso.example")],
        );
        assert_eq!(ctl.take_discovery("ada@contoso.example").len(), 1);
        assert!(ctl.take_discovery("ada@contoso.example").is_empty());
    }

    #[test]
    fn warnings_without_an_engine_are_kept() {
        let ctl = ExchangeCtl::new(ExchangeHooks::default());
        let account = AccountId::new("work");
        ctl.add_warning(&account, ExchangeWarning::RoomsNeedConsent);
        ctl.add_warning(&account, ExchangeWarning::RoomsNeedConsent);
        assert_eq!(
            ctl.warnings_of(&account),
            vec![ExchangeWarning::RoomsNeedConsent]
        );
        assert_eq!(rfc3339(0).as_deref(), Some("1970-01-01T00:00:00Z"));
    }
}
