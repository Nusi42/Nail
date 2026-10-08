//! Server-side rules and vacation replies (ADR 0043 contract, ADR 0044
//! `nail-sieve`, wiring ADR 0048): `GetSieveStatus`, `SyncSieve`,
//! `PreviewSieveScript`, `GetVacation`, `UpdateVacation`, and the upload
//! task.
//!
//! * Every IMAP account with an enabled `sieve` node owns one script on
//!   its ManageSieve server. It is compiled from all of `rules.kdl` plus
//!   the account's vacation reply and uploaded by the upload task whenever
//!   an input changed: rules (saved or edited by hand), the vacation
//!   reply, the account settings, the folders — and at start-up. An upload
//!   is skipped while the inputs match the last successful upload.
//! * Uploading is not an operation-queue write: the user's write is the
//!   rule / vacation change (local first); the upload replays it to the
//!   server, retried after failures (`timing.reconnect_max`), visible as
//!   [`SieveState`].
//! * A rule placed `server` by the last successful upload is skipped by
//!   the arrival rules for that account (manual runs still work), so a
//!   rule never runs twice on a message. The store keeps that list
//!   (`sieve_state.server_rules`) until the script is removed: after a
//!   restart and after failed uploads the old script is still active on
//!   the server, so its rules stay skipped.
//! * The store also keeps where the script was uploaded (host, port, TLS,
//!   script name). Switching ManageSieve off (or removing the node, or the
//!   account) removes Nail's script from that place and re-activates the
//!   user's previous one; a changed host, port or script name removes the
//!   script from the old place before uploading to the new one.
//! * Servers without the `date` extension get the vacation dates applied
//!   by Nail: the script is recompiled on the day the compiler names.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use nail_api::accounts::{AccountConfig, IncomingServer, ServerEndpoint};
use nail_api::rules::Rule;
use nail_api::sieve::{
    ManageSieveConfig, RulePlacement, RuleSieveStatus, SieveAccountStatus, SieveState,
    VacationSettings,
};
use nail_api::{AccountId, ApiError, Event, FolderRole, Request, Response, RuleId};
use nail_sieve::{
    CompileOptions, CompiledScript, FolderMap, FolderResolver, SieveAuth, SieveConfig, SieveError,
    SieveSession, reasons,
};
use nail_store::{SieveLocation, SieveRecord, SieveUpload};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::error::Result;
use crate::service::{Service, lock};

/// What the last successful upload of an account put on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Uploaded {
    /// Digest of the inputs it was compiled from ([`Inputs::digest`]).
    digest: String,
    /// Placement of every rule, as uploaded.
    rules: Vec<RuleSieveStatus>,
    /// Whether the script has an active vacation reply.
    vacation_active: bool,
    /// The day the script must be recompiled (vacation dates applied by
    /// Nail), `YYYY-MM-DD`.
    recompile_on: Option<String>,
}

/// In-memory Sieve state of the core.
#[derive(Default)]
pub(crate) struct SieveCtl {
    uploaded: Mutex<HashMap<AccountId, Uploaded>>,
    uploading: Mutex<HashSet<AccountId>>,
    /// Accounts whose ManageSieve service is missing (last attempt).
    unavailable: Mutex<HashSet<AccountId>>,
    /// Accounts waiting for the upload task (`None` entries: all).
    queued: Mutex<HashSet<AccountId>>,
    /// Accounts whose last attempt failed, with the time to retry.
    retry_at: Mutex<HashMap<AccountId, i64>>,
    wake: Notify,
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::GetSieveStatus { account_id } => {
            let config = service.inner.accounts.config();
            let accounts: Vec<AccountConfig> = match account_id {
                Some(id) => vec![imap_account(&config, &id)?.clone()],
                None => config
                    .accounts
                    .iter()
                    .filter(|a| a.sieve.is_some())
                    .cloned()
                    .collect(),
            };
            let mut statuses = Vec::with_capacity(accounts.len());
            for account in accounts {
                statuses.push(service.sieve_status(&account).await?);
            }
            Response::SieveStatus { accounts: statuses }
        }
        Request::SyncSieve { account_id } => {
            let config = service.inner.accounts.config();
            let account = sieve_account(&config, &account_id)?.clone();
            if !sieve_enabled(&account) {
                return Err(ApiError::unsupported(format!(
                    "ManageSieve is switched off for account {account_id}"
                ))
                .into());
            }
            // Forced: also when the inputs did not change.
            lock(&service.inner.sieve.uploaded).remove(&account_id);
            lock(&service.inner.sieve.retry_at).remove(&account_id);
            lock(&service.inner.sieve.uploading).insert(account_id.clone());
            service.queue_sieve(Some(&account_id));
            service.broadcast(Event::SieveStatusChanged {
                account_id: account_id.clone(),
            });
            Response::SieveStatus {
                accounts: vec![service.sieve_status(&account).await?],
            }
        }
        Request::PreviewSieveScript { account_id } => {
            let config = service.inner.accounts.config();
            let account = sieve_account(&config, &account_id)?.clone();
            let compiled = service.compile_sieve(&account).await?.1;
            Response::SieveScript {
                account_id,
                script: compiled.script,
                rules: compiled.rules,
            }
        }
        Request::GetVacation { account_id } => {
            // M3c: an Exchange account's automatic replies.
            if crate::exchange::exchange_account(service, &account_id).is_ok() {
                return Ok(Response::Vacation {
                    settings: crate::exchange::settings::vacation(service, &account_id).await?,
                    account_id,
                });
            }
            let config = service.inner.accounts.config();
            imap_account(&config, &account_id)?;
            let id = account_id.clone();
            let record = service.store().call(move |s| s.sieve_record(&id)).await?;
            Response::Vacation {
                account_id,
                settings: record.vacation,
            }
        }
        Request::UpdateVacation {
            account_id,
            settings,
        } => {
            if crate::exchange::exchange_account(service, &account_id).is_ok() {
                check_vacation(&settings)?;
                crate::exchange::settings::set_vacation(service, &account_id, &settings).await?;
                return Ok(Response::Vacation {
                    settings: crate::exchange::settings::vacation(service, &account_id).await?,
                    account_id,
                });
            }
            let config = service.inner.accounts.config();
            sieve_account(&config, &account_id)?;
            check_vacation(&settings)?;
            let (id, saved) = (account_id.clone(), settings.clone());
            service
                .store()
                .call(move |s| s.set_vacation(&id, &saved))
                .await?;
            service.queue_sieve(Some(&account_id));
            service.broadcast(Event::SieveStatusChanged {
                account_id: account_id.clone(),
            });
            Response::Vacation {
                account_id,
                settings,
            }
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} is not a Sieve request",
                other.wire_type()
            ))
            .into());
        }
    })
}

/// A configured IMAP account (`not_found` / `unsupported` otherwise).
fn imap_account<'a>(
    config: &'a nail_api::AccountsConfig,
    id: &AccountId,
) -> Result<&'a AccountConfig> {
    let account = config
        .account(id)
        .ok_or_else(|| ApiError::not_found(format!("account {id}")))?;
    if !matches!(account.incoming, IncomingServer::Imap { .. }) {
        return Err(ApiError::unsupported(format!(
            "account {id} is not an IMAP account; Sieve needs IMAP"
        ))
        .into());
    }
    Ok(account)
}

/// An IMAP account with a `sieve` node (`unsupported` otherwise).
fn sieve_account<'a>(
    config: &'a nail_api::AccountsConfig,
    id: &AccountId,
) -> Result<&'a AccountConfig> {
    let account = imap_account(config, id)?;
    if account.sieve.is_none() {
        return Err(ApiError::unsupported(format!(
            "account {id} has no ManageSieve settings (`sieve` in accounts.kdl)"
        ))
        .into());
    }
    Ok(account)
}

/// Whether an account uploads a script.
fn sieve_enabled(account: &AccountConfig) -> bool {
    account.enabled
        && matches!(account.incoming, IncomingServer::Imap { .. })
        && account
            .sieve
            .as_ref()
            .is_some_and(ManageSieveConfig::is_enabled)
}

/// Refuses vacation settings with an issue (the first one named).
fn check_vacation(settings: &VacationSettings) -> Result<()> {
    match settings.issues().first() {
        Some(issue) => Err(ApiError::invalid_localized(
            format!("invalid vacation settings ({})", issue.message_id()),
            issue.message_id(),
        )
        .into()),
        None => Ok(()),
    }
}

/// The account's IMAP server.
fn imap_server(account: &AccountConfig) -> Option<&ServerEndpoint> {
    match &account.incoming {
        IncomingServer::Imap { server, .. } => Some(server),
        IncomingServer::Pop3 { .. }
        | IncomingServer::Gmail { .. }
        | IncomingServer::Exchange { .. } => None,
    }
}

/// Mailbox names from the account's stored folders: paths to the stored
/// folder with that path (the server's own spelling), else `map` (roles to
/// the folder that has them, other paths with `/` as the delimiter).
struct StoreFolders {
    paths: HashSet<String>,
    map: FolderMap,
}

impl FolderResolver for StoreFolders {
    fn mailbox(&self, folder: &nail_api::rules::FolderRef) -> Option<String> {
        match folder {
            nail_api::rules::FolderRef::Path { path, .. } if self.paths.contains(path) => {
                Some(path.clone())
            }
            other => self.map.mailbox(other),
        }
    }
}

/// Everything a script is compiled from.
struct Inputs {
    rules: Vec<Rule>,
    vacation: VacationSettings,
    folders: StoreFolders,
    aliases: Vec<AccountId>,
    own_addresses: Vec<String>,
    utc_offset_minutes: i32,
    today: String,
    endpoint: ServerEndpoint,
    script_name: String,
}

impl Inputs {
    fn options(&self, account: &AccountId, extensions: Vec<String>) -> CompileOptions<'_> {
        CompileOptions {
            account_id: account.clone(),
            account_aliases: self.aliases.clone(),
            extensions,
            folders: &self.folders,
            own_addresses: self.own_addresses.clone(),
            utc_offset_minutes: self.utc_offset_minutes,
            today: Some(self.today.clone()),
            max_redirects: None,
            previous_active: None,
        }
    }

    /// A digest of the inputs: an upload is due when it changes.
    fn digest(&self) -> String {
        let mut hash = Sha256::new();
        for part in [
            serde_json::to_string(&self.rules).unwrap_or_default(),
            serde_json::to_string(&self.vacation).unwrap_or_default(),
            format!("{:?}", sorted(&self.folders.map.roles)),
            format!("{:?}", {
                let mut p: Vec<&String> = self.folders.paths.iter().collect();
                p.sort();
                p
            }),
            format!("{:?}", self.aliases),
            format!("{:?}", self.own_addresses),
            format!("{:?}", self.endpoint),
            self.script_name.clone(),
        ] {
            hash.update(part.as_bytes());
            hash.update([0]);
        }
        hex::encode(hash.finalize())
    }
}

fn sorted(roles: &HashMap<FolderRole, String>) -> Vec<(String, &String)> {
    let mut out: Vec<(String, &String)> =
        roles.iter().map(|(r, n)| (format!("{r:?}"), n)).collect();
    out.sort();
    out
}

impl Service {
    /// Marks accounts for the upload task (`None`: every account).
    pub(crate) fn queue_sieve(&self, account: Option<&AccountId>) {
        let config = self.inner.accounts.config();
        {
            let mut queued = lock(&self.inner.sieve.queued);
            for a in &config.accounts {
                if matches!(a.incoming, IncomingServer::Imap { .. })
                    && account.is_none_or(|id| id == &a.id)
                {
                    queued.insert(a.id.clone());
                }
            }
        }
        self.inner.sieve.wake.notify_one();
        // M3c: Exchange accounts place rules as inbox rules.
        if tokio::runtime::Handle::try_current().is_ok() {
            crate::exchange::settings::queue_rules(self, account);
        }
    }

    /// Rules of an account that its server runs (the arrival rules skip
    /// them): those of the script known to be active there, as recorded
    /// by the last successful upload. Survives restarts and failed
    /// uploads, which leave that script active; empty once it is removed.
    pub(crate) async fn server_rules(&self, account: &AccountId) -> Vec<RuleId> {
        // M3c: rules placed as Exchange inbox rules.
        if crate::exchange::exchange_account(self, account).is_ok() {
            return crate::exchange::settings::placed_rules(self, account).await;
        }
        let id = account.clone();
        match self.store().call(move |s| s.sieve_record(&id)).await {
            Ok(record) if record.script_hash.is_some() => record.server_rules,
            Ok(_) => Vec::new(),
            Err(err) => {
                tracing::warn!(%account, %err, "cannot read the Sieve state");
                Vec::new()
            }
        }
    }

    /// Loads the compile inputs of an account.
    async fn sieve_inputs(&self, account: &AccountConfig) -> Result<(Inputs, SieveRecord)> {
        let server = imap_server(account)
            .ok_or_else(|| ApiError::unsupported("Sieve needs an IMAP account"))?;
        let sieve = account.sieve.clone().unwrap_or_default();
        let config = self.inner.accounts.config();
        let id = account.id.clone();
        let (folders, record) = self
            .store()
            .call(move |s| Ok((s.list_folders(&id)?, s.sieve_record(&id)?)))
            .await?;
        let mut roles = HashMap::new();
        let mut paths = HashSet::new();
        for folder in folders {
            if folder.role != FolderRole::Other && folder.role != FolderRole::Inbox {
                roles
                    .entry(folder.role)
                    .or_insert_with(|| folder.path.clone());
            }
            paths.insert(folder.path);
        }
        let mut aliases: Vec<AccountId> = config
            .virtual_accounts
            .iter()
            .filter(|v| v.members.contains(&account.id))
            .map(|v| v.id.clone())
            .collect();
        aliases.push(AccountId::all());
        let mut own_addresses = vec![account.email.clone()];
        for identity in &account.identities {
            if !own_addresses
                .iter()
                .any(|a| a.eq_ignore_ascii_case(&identity.email))
            {
                own_addresses.push(identity.email.clone());
            }
        }
        let local = nail_triage::Clock::now(&*self.inner.clock).with_timezone(&chrono::Local);
        Ok((
            Inputs {
                rules: self.inner.rules.rules(),
                vacation: record.vacation.clone(),
                folders: StoreFolders {
                    paths,
                    map: FolderMap {
                        roles,
                        delimiter: None,
                    },
                },
                aliases,
                own_addresses,
                utc_offset_minutes: local.offset().local_minus_utc() / 60,
                today: local.format("%Y-%m-%d").to_string(),
                endpoint: sieve.endpoint(server),
                script_name: sieve.effective_script_name().to_owned(),
            },
            record,
        ))
    }

    /// Compiles an account's script against the extensions last seen on
    /// its server (the common ones before the first connection).
    async fn compile_sieve(&self, account: &AccountConfig) -> Result<(Inputs, CompiledScript)> {
        let (inputs, record) = self.sieve_inputs(account).await?;
        let extensions = if record.extensions.is_empty() {
            nail_sieve::COMMON_EXTENSIONS
                .iter()
                .map(|e| (*e).to_owned())
                .collect()
        } else {
            record.extensions.clone()
        };
        let compiled = nail_sieve::compile(
            &inputs.rules,
            &inputs.vacation,
            &inputs.options(&account.id, extensions),
        );
        Ok((inputs, compiled))
    }

    /// The Sieve state of an account.
    async fn sieve_status(&self, account: &AccountConfig) -> Result<SieveAccountStatus> {
        let (inputs, compiled) = if matches!(account.incoming, IncomingServer::Imap { .. }) {
            let (inputs, compiled) = self.compile_sieve(account).await?;
            (Some(inputs), Some(compiled))
        } else {
            (None, None)
        };
        let id = account.id.clone();
        let record = self.store().call(move |s| s.sieve_record(&id)).await?;
        let mut status = account_status(account, record);
        let uploaded = lock(&self.inner.sieve.uploaded).get(&account.id).cloned();
        let current = match (&inputs, &uploaded) {
            (Some(inputs), Some(uploaded)) => uploaded.digest == inputs.digest(),
            _ => false,
        };
        if lock(&self.inner.sieve.uploading).contains(&account.id) {
            status.state = SieveState::Uploading;
        } else if status.state != SieveState::Disabled
            && lock(&self.inner.sieve.unavailable).contains(&account.id)
        {
            status.state = SieveState::Unavailable;
        } else if status.state == SieveState::InSync && !current {
            status.state = SieveState::Pending;
        }
        status.vacation_active = status.state == SieveState::InSync
            && uploaded.as_ref().is_some_and(|u| u.vacation_active);
        status.rules = match (status.state, uploaded) {
            (SieveState::InSync, Some(uploaded)) => uploaded.rules,
            (state, _) => {
                let mut rules = compiled.map(|c| c.rules).unwrap_or_default();
                let reason = match state {
                    SieveState::Disabled => reasons::SIEVE_OFF,
                    SieveState::Unavailable => reasons::UNAVAILABLE,
                    _ => reasons::PENDING,
                };
                nail_sieve::demote_server_rules(&mut rules, reason);
                rules
            }
        };
        Ok(status)
    }

    /// Uploads (or removes) one account's script when needed. Returns
    /// whether the server was contacted.
    async fn sync_sieve_account(&self, account: &AccountConfig) -> bool {
        let id = account.id.clone();
        if !sieve_enabled(account) {
            let recorded = {
                let lookup = id.clone();
                self.store()
                    .call(move |s| s.sieve_record(&lookup))
                    .await
                    .ok()
            };
            lock(&self.inner.sieve.uploaded).remove(&id);
            if let Some(record) = recorded
                && record.script_hash.is_some()
                && account.enabled
            {
                if !self.remove_sieve_script(account, record.location).await {
                    let retry = self.triage_now()
                        + i64::try_from(self.inner.timing.reconnect_max.as_secs()).unwrap_or(300);
                    lock(&self.inner.sieve.retry_at).insert(id, retry);
                }
                return true;
            }
            return false;
        }
        let (inputs, record) = match self.sieve_inputs(account).await {
            Ok(loaded) => loaded,
            Err(err) => {
                tracing::warn!(account = %id, %err, "cannot prepare the Sieve script");
                return false;
            }
        };
        let here = location(&inputs.endpoint, &inputs.script_name);
        let digest = inputs.digest();
        let forced = lock(&self.inner.sieve.uploading).contains(&id);
        let unchanged = lock(&self.inner.sieve.uploaded)
            .get(&id)
            .is_some_and(|u| u.digest == digest);
        let today = inputs.today.clone();
        let recompile_due = lock(&self.inner.sieve.uploaded)
            .get(&id)
            .and_then(|u| u.recompile_on.clone())
            .is_some_and(|day| day <= today);
        if unchanged && !forced && !recompile_due {
            return false;
        }
        lock(&self.inner.sieve.uploading).insert(id.clone());
        self.broadcast(Event::SieveStatusChanged {
            account_id: id.clone(),
        });
        // The host, port or script name changed since the last upload:
        // Nail's script leaves the old place first (best effort; an old
        // server that is gone for good must not block the new one).
        if let Some(old) = record.location.filter(|old| *old != here)
            && record.script_hash.is_some()
        {
            tracing::info!(account = %id, old = ?old, new = ?here, "the Sieve script moves");
            self.remove_sieve_script(account, Some(old)).await;
        }
        let outcome = self.upload_sieve(account, &inputs).await;
        lock(&self.inner.sieve.uploading).remove(&id);
        let now = self.triage_now();
        match outcome {
            Ok(report) => {
                lock(&self.inner.sieve.unavailable).remove(&id);
                lock(&self.inner.sieve.retry_at).remove(&id);
                tracing::info!(
                    account = %id,
                    server_rules = report.compiled.server_rules().len(),
                    activated = report.activated,
                    "Sieve script uploaded"
                );
                let (script, extensions, others) = (
                    report.compiled.script.clone(),
                    report.extensions.clone(),
                    report.other_scripts.clone(),
                );
                let server_rules: Vec<RuleId> = report
                    .compiled
                    .rules
                    .iter()
                    .filter(|r| r.placement == RulePlacement::Server)
                    .map(|r| r.rule_id.clone())
                    .collect();
                let account_id = id.clone();
                if let Err(err) = self
                    .store()
                    .call(move |s| {
                        s.record_sieve_upload(
                            &account_id,
                            &SieveUpload {
                                script: &script,
                                extensions: &extensions,
                                other_scripts: &others,
                                location: &here,
                                server_rules: &server_rules,
                                now,
                            },
                        )
                    })
                    .await
                {
                    tracing::warn!(%err, "cannot record the Sieve upload");
                }
                lock(&self.inner.sieve.uploaded).insert(
                    id.clone(),
                    Uploaded {
                        digest,
                        rules: report.compiled.rules.clone(),
                        vacation_active: report.compiled.vacation.in_script(),
                        recompile_on: report.compiled.vacation.recompile_on.clone(),
                    },
                );
            }
            Err(err) => {
                tracing::warn!(account = %id, %err, "Sieve upload failed");
                let unavailable = err.is_unavailable();
                if unavailable {
                    lock(&self.inner.sieve.unavailable).insert(id.clone());
                } else {
                    lock(&self.inner.sieve.unavailable).remove(&id);
                }
                lock(&self.inner.sieve.uploaded).remove(&id);
                let retry =
                    now + i64::try_from(self.inner.timing.reconnect_max.as_secs()).unwrap_or(300);
                lock(&self.inner.sieve.retry_at).insert(id.clone(), retry);
                let api = ApiError::from(nail_api::engine::EngineError::from(err));
                let account_id = id.clone();
                if let Err(err) = self
                    .store()
                    .call(move |s| s.record_sieve_error(&account_id, Some(&api)))
                    .await
                {
                    tracing::warn!(%err, "cannot record the Sieve failure");
                }
            }
        }
        self.broadcast(Event::SieveStatusChanged { account_id: id });
        true
    }

    /// Logs in to an account's ManageSieve server (a rejected OAuth2
    /// token is refreshed once).
    async fn sieve_login(
        &self,
        account: &AccountConfig,
        endpoint: &ServerEndpoint,
    ) -> std::result::Result<SieveSession, SieveError> {
        let server = imap_server(account)
            .ok_or_else(|| SieveError::NotManageSieve("not an IMAP account".to_owned()))?;
        let auth = SieveAuth::from_config(
            &account.id,
            &server.auth,
            std::sync::Arc::clone(&self.inner.secrets),
            self.inner.tokens.clone(),
        )
        .map_err(|e| SieveError::AuthUnsupported(e.message))?;
        let config = SieveConfig::from_endpoint(endpoint).with_tap(self.account_tap(&account.id));
        let credentials = auth
            .credentials(false)
            .await
            .map_err(|e| SieveError::AuthFailed(e.message))?;
        match SieveSession::login(&config, &credentials).await {
            Err(SieveError::AuthFailed(reason))
                if matches!(credentials, nail_sieve::SieveCredentials::OAuth2 { .. }) =>
            {
                tracing::info!(%reason, "ManageSieve rejected the access token; refreshing it once");
                let credentials = auth
                    .credentials(true)
                    .await
                    .map_err(|e| SieveError::AuthFailed(e.message))?;
                SieveSession::login(&config, &credentials).await
            }
            other => other,
        }
    }

    async fn upload_sieve(
        &self,
        account: &AccountConfig,
        inputs: &Inputs,
    ) -> std::result::Result<nail_sieve::UploadReport, SieveError> {
        let mut session = self.sieve_login(account, &inputs.endpoint).await?;
        let report = nail_sieve::upload(
            &mut session,
            &inputs.script_name,
            &inputs.rules,
            &inputs.vacation,
            inputs.options(&account.id, Vec::new()),
        )
        .await;
        if let Err(err) = session.logout().await {
            tracing::debug!(%err, "ManageSieve logout failed");
        }
        report
    }

    /// Removes Nail's script from where it was uploaded (`location`, as
    /// recorded; the current settings for uploads recorded before schema
    /// v8). Returns whether it is gone.
    async fn remove_sieve_script(
        &self,
        account: &AccountConfig,
        location: Option<SieveLocation>,
    ) -> bool {
        let Some(server) = imap_server(account) else {
            return false;
        };
        let (endpoint, script_name) = if let Some(at) = location {
            (
                ServerEndpoint {
                    host: at.host,
                    port: at.port,
                    tls: at.tls,
                    auth: server.auth.clone(),
                },
                at.script_name,
            )
        } else {
            let sieve = account.sieve.clone().unwrap_or_default();
            (
                sieve.endpoint(server),
                sieve.effective_script_name().to_owned(),
            )
        };
        let result = async {
            let mut session = self.sieve_login(account, &endpoint).await?;
            let restored = nail_sieve::remove(&mut session, &script_name).await;
            drop(session.logout().await);
            restored
        }
        .await;
        let removed = match result {
            Ok(restored) => {
                tracing::info!(account = %account.id, ?restored, "Sieve script removed");
                let id = account.id.clone();
                if let Err(err) = self.store().call(move |s| s.clear_sieve_upload(&id)).await {
                    tracing::warn!(%err, "cannot record the removed Sieve script");
                }
                true
            }
            Err(err) => {
                tracing::warn!(account = %account.id, %err, "cannot remove the Sieve script");
                false
            }
        };
        self.broadcast(Event::SieveStatusChanged {
            account_id: account.id.clone(),
        });
        removed
    }

    /// Before an account is deleted: removes Nail's script from its server
    /// (bounded; a failure is logged and does not stop the deletion).
    pub(crate) async fn remove_sieve_before_delete(&self, account: &AccountConfig) {
        if !matches!(account.incoming, IncomingServer::Imap { .. }) {
            return;
        }
        let id = account.id.clone();
        let Ok(record) = self.store().call(move |s| s.sieve_record(&id)).await else {
            return;
        };
        if record.script_hash.is_none() {
            return;
        }
        let limit = std::time::Duration::from_secs(20);
        if tokio::time::timeout(limit, self.remove_sieve_script(account, record.location))
            .await
            .is_err()
        {
            tracing::warn!(account = %account.id, "removing the Sieve script timed out");
        }
        lock(&self.inner.sieve.uploaded).remove(&account.id);
    }
}

/// The upload task: uploads queued accounts, retries failures, and
/// recompiles vacation scripts on their day.
pub(crate) async fn run_sieve(service: Service) {
    service.queue_sieve(None);
    let tick = std::time::Duration::from_mins(1).min(service.inner.timing.reconnect_max);
    loop {
        let now = service.triage_now();
        let mut due: HashSet<AccountId> = std::mem::take(&mut *lock(&service.inner.sieve.queued));
        {
            let mut retry = lock(&service.inner.sieve.retry_at);
            let ready: Vec<AccountId> = retry
                .iter()
                .filter(|(_, at)| **at <= now)
                .map(|(id, _)| id.clone())
                .collect();
            for id in ready {
                retry.remove(&id);
                due.insert(id);
            }
        }
        // Vacation dates applied by Nail: recompile on the named day.
        let today = nail_triage::Clock::now(&*service.inner.clock)
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string();
        for (id, uploaded) in lock(&service.inner.sieve.uploaded).iter() {
            if uploaded.recompile_on.as_ref().is_some_and(|d| *d <= today) {
                due.insert(id.clone());
            }
        }
        let config = service.inner.accounts.config();
        for account in config.accounts.iter().filter(|a| due.contains(&a.id)) {
            service.sync_sieve_account(account).await;
        }
        // Forced uploads of accounts that vanished meanwhile end.
        lock(&service.inner.sieve.uploading).retain(|id| config.account(id).is_some());
        tokio::select! {
            () = tokio::time::sleep(tick) => {}
            () = service.inner.sieve.wake.notified() => {}
        }
    }
}

/// Where an upload goes: the endpoint and the script name.
fn location(endpoint: &ServerEndpoint, script_name: &str) -> SieveLocation {
    SieveLocation {
        host: endpoint.host.clone(),
        port: endpoint.port,
        tls: endpoint.tls,
        script_name: script_name.to_owned(),
    }
}

/// Pure part of the status: the state from the settings and the store.
pub(crate) fn account_status(account: &AccountConfig, record: SieveRecord) -> SieveAccountStatus {
    let config = account.sieve.clone().unwrap_or_default();
    let enabled = sieve_enabled(account);
    let state = if !enabled {
        SieveState::Disabled
    } else if record.last_error.is_some() {
        SieveState::Failed
    } else if record.script_hash.is_some() {
        SieveState::InSync
    } else {
        SieveState::Pending
    };
    let error = if enabled {
        record.last_error.clone()
    } else {
        None
    };
    SieveAccountStatus {
        account_id: account.id.clone(),
        state,
        script_name: config.effective_script_name().to_owned(),
        uploaded_at: record.uploaded_at.map(nail_util::time::rfc3339_utc),
        extensions: record.extensions,
        other_scripts: record.other_scripts,
        rules: Vec::new(),
        vacation_active: record.script_hash.is_some() && record.vacation.enabled,
        error: error.as_ref().map(|e| e.code),
        message: error.as_ref().map(|e| e.message.clone()),
        localized: error.and_then(|e| e.localized).map(|l| *l),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::ErrorCode;

    fn account(sieve: Option<ManageSieveConfig>) -> AccountConfig {
        AccountConfig {
            sieve,
            ..nail_api::testing::imap_account()
        }
    }

    #[test]
    fn states_follow_settings_and_records() {
        let off = account_status(&account(None), SieveRecord::default());
        assert_eq!(off.state, SieveState::Disabled);
        assert_eq!(off.script_name, "nail");

        let on = account(Some(ManageSieveConfig::default()));
        assert_eq!(
            account_status(&on, SieveRecord::default()).state,
            SieveState::Pending
        );
        let uploaded = SieveRecord {
            script_hash: Some("0".repeat(64)),
            uploaded_at: Some(1_790_000_000),
            vacation: VacationSettings {
                enabled: true,
                body: "Away".into(),
                ..VacationSettings::default()
            },
            ..SieveRecord::default()
        };
        let status = account_status(&on, uploaded.clone());
        assert_eq!(status.state, SieveState::InSync);
        assert_eq!(status.uploaded_at.as_deref(), Some("2026-09-21T14:13:20Z"));

        let failed = SieveRecord {
            last_error: Some(
                ApiError::new(ErrorCode::Network, "refused")
                    .with_localized(nail_api::LocalizedMessage::new("sieve-error-connect")),
            ),
            ..uploaded
        };
        let status = account_status(&on, failed);
        assert_eq!(status.state, SieveState::Failed);
        assert_eq!(status.error, Some(ErrorCode::Network));
        assert_eq!(
            status.localized.map(|l| l.id).as_deref(),
            Some("sieve-error-connect")
        );

        let disabled = account(Some(ManageSieveConfig {
            enabled: Some(false),
            script_name: Some("mine".into()),
            ..ManageSieveConfig::default()
        }));
        let status = account_status(&disabled, SieveRecord::default());
        assert_eq!(status.state, SieveState::Disabled);
        assert_eq!(status.script_name, "mine");
    }

    #[test]
    fn folders_resolve_to_the_stored_names() {
        use nail_api::rules::FolderRef;
        let folders = StoreFolders {
            paths: HashSet::from(["Lists.Rust".to_owned()]),
            map: FolderMap::new(None, &[(FolderRole::Junk, "Spam")]),
        };
        let role = |role| FolderRef::Role {
            account_id: None,
            role,
        };
        let path = |p: &str| FolderRef::Path {
            account_id: None,
            path: p.into(),
        };
        assert_eq!(
            folders.mailbox(&role(FolderRole::Inbox)).as_deref(),
            Some("INBOX")
        );
        assert_eq!(
            folders.mailbox(&role(FolderRole::Junk)).as_deref(),
            Some("Spam")
        );
        assert_eq!(folders.mailbox(&role(FolderRole::Archive)), None);
        assert_eq!(
            folders.mailbox(&path("Lists.Rust")).as_deref(),
            Some("Lists.Rust")
        );
        assert_eq!(
            folders.mailbox(&path("Receipts/2026")).as_deref(),
            Some("Receipts/2026")
        );
    }
}
