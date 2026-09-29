//! Filter rules (M1b WP22, ADR 0024): `rules.kdl`, the rule requests, and
//! running rules through the operation queue.
//!
//! * [`RulesManager`] owns `rules.kdl` like the account manager owns
//!   `accounts.kdl`: the format-preserving [`RulesDocument`] for edits
//!   (`SaveRule`, `DeleteRule`, `ReorderRules`), the compiled
//!   [`nail_rules::RuleSet`], and every error of a broken file (the last
//!   valid rules stay active).
//! * Messages are shown to `nail-rules` as [`MessageData`] built from the
//!   store (summary, decoded header list, body text, attachments) and the
//!   accounts / folders as a [`StaticMailbox`].
//! * Running: arrival rules for new mail (the sync engines put new
//!   messages into the store's rule inbox, which survives a restart),
//!   on-send rules for the Sent copy, scheduled rules (`schedule`,
//!   `older-than`) on a timer, and manual runs (`RunRule`). Every planned
//!   queue operation is enqueued and recorded with its inverse, so a run is
//!   undone as one unit (`UndoRuleRun`). Copies, forwards, redirects and
//!   replies are carried out directly and cannot be undone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nail_api::rules::{Rule, RuleMatch, RuleRun, RuleRunCause, RuleRunState};
use nail_api::{
    Address, ApiError, ConfigIssue, ErrorCode, Event, FolderId, FolderRole, MessageId,
    MessageSummary, Request, Response, RuleId, RuleRunId,
};
use nail_config::ConfigError;
use nail_config::rules::{RULES_FILE, RulesDocument};
use nail_rules::{
    AccountInfo, Attachment, CompiledRule, Effect, Env, FolderInfo, MessageData, MessageOutcome,
    PlannedAction, RuleSet, StaticMailbox,
};
use nail_store::{BlobStore, RuleRunAction, RuleRunRecord, RuleState, Store};

use crate::engine::unix_now;
use crate::error::Result;
use crate::service::Service;

/// Messages a dry run or a manual run over a folder looks at, newest first.
const SCAN_LIMIT: u32 = 2000;

/// New messages taken from the rule inbox per pass.
const INBOX_BATCH: u32 = 200;

/// Longest run history page.
const MAX_RUNS: u32 = 500;

/// What [`RulesManager::reload`] found.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RulesReload {
    /// Same rules as before.
    Unchanged,
    /// New valid rules (`was_invalid`: the file had errors before).
    Changed { was_invalid: bool },
    /// The file is invalid now; the last valid rules stay.
    Invalid,
}

struct State {
    doc: RulesDocument,
    /// Every error of the file on disk (empty when valid).
    errors: Vec<ConfigError>,
    set: Arc<RuleSet>,
}

/// Owns `rules.kdl`.
pub(crate) struct RulesManager {
    dir: PathBuf,
    state: Mutex<State>,
}

fn io_errors(err: &std::io::Error) -> Vec<ConfigError> {
    vec![ConfigError::io(RULES_FILE, err)]
}

fn first_message(errors: &[ConfigError]) -> String {
    errors
        .first()
        .map_or_else(|| "invalid rule".to_owned(), ToString::to_string)
}

impl RulesManager {
    /// Loads `<dir>/rules.kdl` (missing = no rules).
    pub(crate) fn load(dir: &Path) -> Self {
        let (doc, errors) = match RulesDocument::load(dir) {
            Ok(doc) => (doc, Vec::new()),
            Err(errors) => {
                tracing::warn!(error = %first_message(&errors), "invalid rules.kdl; no rules active");
                (RulesDocument::empty(RULES_FILE), errors)
            }
        };
        let set = Arc::new(RuleSet::new(doc.rules().to_vec()));
        Self {
            dir: dir.to_owned(),
            state: Mutex::new(State { doc, errors, set }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The rules in file order.
    pub(crate) fn rules(&self) -> Vec<Rule> {
        self.state().doc.rules().to_vec()
    }

    /// The compiled rules.
    pub(crate) fn set(&self) -> Arc<RuleSet> {
        Arc::clone(&self.state().set)
    }

    /// Every error of the file on disk (empty when valid).
    pub(crate) fn errors(&self) -> Vec<ConfigError> {
        self.state().errors.clone()
    }

    /// Applies `edit` to a copy of the document, saves it and makes it
    /// current. A hand edit not reloaded yet is picked up first; a broken
    /// file on disk refuses the edit (`config`).
    pub(crate) fn edit<R>(&self, edit: impl FnOnce(&mut RulesDocument) -> Result<R>) -> Result<R> {
        let mut state = self.state();
        let on_disk = match std::fs::read_to_string(self.dir.join(RULES_FILE)) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(ConfigError::io(RULES_FILE, &e).into()),
        };
        if on_disk.as_deref().unwrap_or_default() != state.doc.text() {
            match on_disk
                .as_deref()
                .map(|t| RulesDocument::parse(t, RULES_FILE))
            {
                Some(Ok(doc)) => state.doc = doc,
                None => state.doc = RulesDocument::empty(RULES_FILE),
                Some(Err(errors)) => {
                    return Err(ApiError::new(
                        ErrorCode::Config,
                        format!(
                            "rules.kdl is invalid; fix it first ({})",
                            first_message(&errors)
                        ),
                    )
                    .into());
                }
            }
            state.errors.clear();
        }
        let mut doc = state.doc.clone();
        let out = edit(&mut doc)?;
        doc.save(&self.dir)?;
        state.set = Arc::new(RuleSet::new(doc.rules().to_vec()));
        state.doc = doc;
        state.errors.clear();
        Ok(out)
    }

    /// Reads the file again after a change on disk.
    pub(crate) fn reload(&self) -> RulesReload {
        let loaded = match std::fs::read_to_string(self.dir.join(RULES_FILE)) {
            Ok(text) => RulesDocument::parse(&text, RULES_FILE),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(RulesDocument::empty(RULES_FILE))
            }
            Err(e) => Err(io_errors(&e)),
        };
        let mut state = self.state();
        match loaded {
            Ok(doc) => {
                let was_invalid = !state.errors.is_empty();
                let changed = doc.rules() != state.doc.rules();
                state.errors.clear();
                if changed {
                    state.set = Arc::new(RuleSet::new(doc.rules().to_vec()));
                }
                state.doc = doc;
                if changed || was_invalid {
                    RulesReload::Changed { was_invalid }
                } else {
                    RulesReload::Unchanged
                }
            }
            Err(errors) => {
                if state.errors == errors {
                    RulesReload::Unchanged
                } else {
                    state.errors = errors;
                    RulesReload::Invalid
                }
            }
        }
    }
}

/// A rule-file error as an `invalid_request`.
fn rule_error(errors: &[ConfigError]) -> crate::CoreError {
    ApiError::invalid_request(first_message(errors)).into()
}

/// Every problem `nail-rules` finds in a rule, as request errors.
fn check_rule(rule: &Rule) -> Result<()> {
    let issues = nail_rules::validate(rule);
    if issues.is_empty() {
        return Ok(());
    }
    let text = issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    Err(ApiError::invalid_request(text).into())
}

fn rfc3339(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A stored run as the protocol shows it.
fn to_run(record: RuleRunRecord) -> RuleRun {
    RuleRun {
        id: record.id,
        rule_id: record.rule_id,
        rule_name: record.rule_name,
        cause: record.cause,
        state: record.state,
        started_at: rfc3339(record.started_at),
        finished_at: record.finished_at.map(rfc3339),
        matched: record.matched,
        op_ids: record.op_ids.into_iter().map(crate::ids::op_id).collect(),
        undoable: record.undoable,
        message: record.message,
    }
}

/// The local time zone's offset east of UTC, in seconds.
fn utc_offset_secs() -> i32 {
    chrono::Local::now().offset().local_minus_utc()
}

/// Accounts and folders as rules see them.
fn mailbox(store: &Store, config: &nail_api::AccountsConfig) -> nail_store::Result<StaticMailbox> {
    let mut mailbox = StaticMailbox::default();
    for account in store.list_accounts()? {
        for folder in store.list_folders(&account.id)? {
            mailbox.folders.push(FolderInfo {
                id: folder.id,
                account_id: folder.account_id,
                path: folder.path,
                name: folder.name,
                role: folder.role,
            });
        }
        mailbox.accounts.push(AccountInfo {
            id: account.id,
            name: account.name,
            email: account.email,
            members: Vec::new(),
        });
    }
    for va in &config.virtual_accounts {
        mailbox.accounts.push(AccountInfo {
            id: va.id.clone(),
            name: va.name.clone(),
            email: String::new(),
            members: va.members.clone(),
        });
    }
    Ok(mailbox)
}

/// A stored message as rules see it, with its summary. `None` when the
/// message is gone.
pub(crate) fn message_view(
    store: &Store,
    blobs: &BlobStore,
    id: &MessageId,
) -> Option<(MessageData, MessageSummary)> {
    let stored = store.get_message(id).ok()?;
    let summary = stored.summary.clone();
    let mut data = MessageData::new(
        summary.id.clone(),
        summary.account_id.clone(),
        summary.folder_id.clone(),
    );
    data.subject.clone_from(&summary.subject);
    data.from = summary.from.iter().cloned().collect();
    data.to.clone_from(&summary.to);
    data.flags = summary.flags;
    data.tags.clone_from(&summary.tags);
    data.date = chrono::DateTime::parse_from_rfc3339(&summary.date)
        .map(|d| d.timestamp())
        .unwrap_or(0);
    if let Some(raw) = stored.blob_hash.as_ref().and_then(|h| blobs.get(h).ok()) {
        let parsed = nail_mime::parse_message(&raw);
        data.headers = nail_mime::header_fields(&raw);
        data.from = parsed.headers.from.clone();
        data.to = parsed.headers.to.clone();
        data.cc = parsed.headers.cc.clone();
        data.reply_to = parsed.headers.reply_to.clone();
        data.sender = data
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("Sender"))
            .map(|(_, v)| parse_addresses(v))
            .unwrap_or_default();
        data.body = Some(parsed.text.clone());
        data.attachments = parsed
            .attachments
            .iter()
            .map(|a| Attachment {
                filename: a.filename.clone(),
                mime: a.mime.to_ascii_lowercase(),
            })
            .collect();
        data.size = raw.len() as u64;
    }
    Some((data, summary))
}

/// Addresses of a raw header value (`Name <a@b>, c@d`), best effort.
fn parse_addresses(value: &str) -> Vec<Address> {
    value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let (name, email) = match (part.rfind('<'), part.rfind('>')) {
                (Some(start), Some(end)) if start < end => {
                    let name = part[..start].trim().trim_matches('"').trim();
                    (
                        (!name.is_empty()).then(|| name.to_owned()),
                        part[start + 1..end].trim().to_owned(),
                    )
                }
                _ => (None, part.to_owned()),
            };
            email.contains('@').then_some(Address { name, email })
        })
        .collect()
}

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::ListRules => Response::Rules {
            rules: service.inner.rules.rules(),
        },
        Request::SaveRule { rule, position } => {
            check_rule(&rule)?;
            let position = position.map(|p| p as usize);
            let saved = service.edit_rules(move |doc| {
                doc.upsert_rule(*rule, position)
                    .map_err(|errors| rule_error(&errors))
            })?;
            Response::RuleSaved {
                rule: Box::new(saved),
            }
        }
        Request::DeleteRule { rule_id } => {
            let id = rule_id.clone();
            let removed = service.edit_rules(move |doc| {
                doc.remove_rule(&id).map_err(|errors| rule_error(&errors))
            })?;
            if !removed {
                return Err(ApiError::not_found(format!("rule {rule_id}")).into());
            }
            Response::Done
        }
        Request::ReorderRules { rule_ids } => {
            service.edit_rules(move |doc| {
                doc.reorder(&rule_ids).map_err(|errors| rule_error(&errors))
            })?;
            Response::Done
        }
        Request::RuleToText { rule } => Response::RuleText {
            text: nail_config::rules::rule_to_text(&rule),
        },
        Request::RuleFromText { text } => match nail_config::rules::parse_rule_text(&text) {
            Ok(rule) => {
                let (line, column) = rule.source.as_ref().map_or((1, 1), |s| (s.line, s.column));
                let errors = nail_rules::validate(&rule)
                    .into_iter()
                    .map(|issue| ConfigIssue {
                        line,
                        column,
                        message: issue.to_string(),
                    })
                    .collect();
                Response::RuleParsed {
                    rule: Some(Box::new(rule)),
                    errors,
                }
            }
            Err(errors) => Response::RuleParsed {
                rule: None,
                errors: errors.iter().map(ConfigError::to_issue).collect(),
            },
        },
        Request::TestRule {
            rule,
            folder_id,
            limit,
        } => service.test_rule(*rule, folder_id, limit).await?,
        Request::RunRule {
            rule_id,
            message_ids,
            folder_id,
        } => Response::RuleRan {
            run: Box::new(
                service
                    .run_rule_now(&rule_id, message_ids, folder_id)
                    .await?,
            ),
        },
        Request::ListRuleRuns { rule_id, limit } => {
            let limit = limit.clamp(1, MAX_RUNS);
            let records = service
                .inner
                .store
                .call(move |s| s.list_rule_runs(rule_id.as_ref(), limit))
                .await?;
            Response::RuleRuns {
                runs: records.into_iter().map(to_run).collect(),
            }
        }
        Request::UndoRuleRun { run_id } => Response::Queued {
            op_ids: service.undo_rule_run(&run_id).await?,
        },
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to the rules module",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

/// One run being recorded.
struct OpenRun {
    id: RuleRunId,
    matched: u32,
    failure: Option<String>,
}

impl Service {
    fn edit_rules<R>(&self, edit: impl FnOnce(&mut RulesDocument) -> Result<R>) -> Result<R> {
        let out = self.inner.rules.edit(edit)?;
        self.broadcast(Event::RulesChanged);
        let ids: Vec<RuleId> = self.inner.rules.rules().into_iter().map(|r| r.id).collect();
        let store = self.inner.store.clone();
        std::thread::spawn(move || {
            if let Err(err) = store.retain_rule_states(&ids) {
                tracing::debug!(%err, "cannot prune rule states");
            }
        });
        Ok(out)
    }

    /// Re-reads `rules.kdl` after a change on disk and reports it.
    pub(crate) fn reload_rules_file(&self) {
        match self.inner.rules.reload() {
            RulesReload::Unchanged => {}
            RulesReload::Changed { was_invalid } => {
                tracing::info!("rules.kdl changed");
                if was_invalid {
                    self.broadcast(nail_config::issues_event(RULES_FILE, &[]));
                }
                self.broadcast(Event::RulesChanged);
            }
            RulesReload::Invalid => {
                let errors = self.inner.rules.errors();
                tracing::warn!(error = %first_message(&errors), "invalid rules.kdl; keeping the last valid rules");
                if let Some(first) = errors.first() {
                    self.broadcast(first.to_event());
                }
                self.broadcast(nail_config::issues_event(RULES_FILE, &errors));
            }
        }
    }

    /// The messages a rule looks at when no message is named: the folder,
    /// or the Inbox (and every folder a folder scope names) of every
    /// account; newest first.
    async fn candidate_messages(
        &self,
        folder_id: Option<FolderId>,
        rule: &Rule,
    ) -> Result<Vec<MessageId>> {
        if let Some(folder) = folder_id {
            let page = self.list_messages(&folder, 0, SCAN_LIMIT).await?;
            return Ok(page.messages.into_iter().map(|m| m.id).collect());
        }
        let wants_folders: Vec<nail_api::rules::FolderRef> = rule
            .scope
            .iter()
            .filter_map(|s| match s {
                nail_api::rules::RuleScope::Folder { folder } => Some(folder.clone()),
                nail_api::rules::RuleScope::Account { .. } => None,
            })
            .collect();
        let config = self.inner.accounts.config();
        let ids = self
            .inner
            .store
            .call(move |s| {
                let mailbox = mailbox(s, &config)?;
                let env = Env {
                    now: unix_now(),
                    utc_offset_secs: 0,
                    mailbox: &mailbox,
                };
                let mut folders: Vec<FolderId> = Vec::new();
                for account in s.list_accounts()? {
                    for folder in s.list_folders(&account.id)? {
                        let named = wants_folders.iter().any(|f| {
                            env.resolve_folder(f, &account.id)
                                .is_some_and(|info| info.id == folder.id)
                        });
                        if named || (wants_folders.is_empty() && folder.role == FolderRole::Inbox) {
                            folders.push(folder.id);
                        }
                    }
                }
                let mut summaries: Vec<MessageSummary> = Vec::new();
                for folder in folders {
                    summaries.extend(s.list_messages(&folder, 0, SCAN_LIMIT)?.messages);
                }
                summaries.sort_by(|a, b| b.date.cmp(&a.date));
                summaries.truncate(SCAN_LIMIT as usize);
                Ok(summaries.into_iter().map(|m| m.id).collect())
            })
            .await?;
        Ok(ids)
    }

    /// Loads messages as rules see them (skipping vanished ones), with the
    /// accounts and folders around them.
    async fn load_views(
        &self,
        ids: Vec<MessageId>,
    ) -> Result<(Vec<(MessageData, MessageSummary)>, StaticMailbox)> {
        let store = self.inner.store.clone();
        let blobs = self.inner.blobs.clone();
        let config = self.inner.accounts.config();
        tokio::task::spawn_blocking(move || -> Result<_> {
            let mailbox = mailbox(&store, &config)?;
            let views = ids
                .iter()
                .filter_map(|id| message_view(&store, &blobs, id))
                .collect();
            Ok((views, mailbox))
        })
        .await?
    }

    /// `TestRule`: a dry run.
    async fn test_rule(
        &self,
        rule: Rule,
        folder_id: Option<FolderId>,
        limit: u32,
    ) -> Result<Response> {
        check_rule(&rule)?;
        let ids = self.candidate_messages(folder_id, &rule).await?;
        let compiled = CompiledRule::new(rule).map_err(|issues| {
            ApiError::invalid_request(
                issues
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        let (views, mailbox) = self.load_views(ids).await?;
        let env = Env {
            now: unix_now(),
            utc_offset_secs: utc_offset_secs(),
            mailbox: &mailbox,
        };
        let limit = limit.clamp(1, SCAN_LIMIT) as usize;
        let report = nail_rules::test_rule(
            &compiled,
            views.iter().map(|(d, _)| d as &dyn nail_rules::MessageView),
            &env,
            limit,
        );
        let summaries: HashMap<&MessageId, &MessageSummary> =
            views.iter().map(|(_, s)| (&s.id, s)).collect();
        let matches = report
            .matches
            .into_iter()
            .filter_map(|m| {
                Some(RuleMatch {
                    message: (*summaries.get(&m.message_id)?).clone(),
                    actions: m.actions,
                    explanation: m
                        .hits
                        .into_iter()
                        .map(|h| nail_api::rules::ConditionHit {
                            path: h.path,
                            matched: h.matched,
                            value: h.value,
                        })
                        .collect(),
                })
            })
            .collect();
        Ok(Response::RuleTested {
            scanned: report.scanned,
            matches,
        })
    }

    /// `RunRule`: runs one rule now on the named messages, the folder, or
    /// its default candidates.
    async fn run_rule_now(
        &self,
        rule_id: &RuleId,
        message_ids: Vec<MessageId>,
        folder_id: Option<FolderId>,
    ) -> Result<RuleRun> {
        let set = self.inner.rules.set();
        let Some(compiled) = set.get(rule_id) else {
            return Err(match set.broken().iter().find(|(id, _)| id == rule_id) {
                Some((_, issues)) => ApiError::invalid_request(
                    issues
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; "),
                ),
                None => ApiError::not_found(format!("rule {rule_id}")),
            }
            .into());
        };
        let ids = if message_ids.is_empty() {
            self.candidate_messages(folder_id, compiled.rule()).await?
        } else {
            message_ids
        };
        let run = self
            .run_one_rule(compiled, RuleRunCause::Manual, ids)
            .await?;
        Ok(run)
    }

    /// Runs one rule on messages and records the run (also when nothing
    /// matched, so a manual or scheduled run shows up in the history).
    async fn run_one_rule(
        &self,
        compiled: &CompiledRule,
        cause: RuleRunCause,
        ids: Vec<MessageId>,
    ) -> Result<RuleRun> {
        let (views, mailbox) = self.load_views(ids).await?;
        let outcomes = {
            let env = Env {
                now: unix_now(),
                utc_offset_secs: utc_offset_secs(),
                mailbox: &mailbox,
            };
            nail_rules::run_rule(
                compiled,
                cause,
                views.iter().map(|(d, _)| d as &dyn nail_rules::MessageView),
                &env,
            )
        };
        let rule = compiled.rule();
        let (id, name) = (rule.id.clone(), rule.name.clone());
        let started = self
            .inner
            .store
            .call(move |s| s.start_rule_run(&id, &name, cause, unix_now()))
            .await?;
        let mut runs = HashMap::from([(
            rule.id.clone(),
            OpenRun {
                id: started,
                matched: 0,
                failure: None,
            },
        )]);
        self.execute_outcomes(cause, &outcomes, &mut runs).await;
        let finished = self.finish_runs(runs).await;
        finished
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::internal("the rule run was not recorded").into())
    }

    /// Carries out planned actions, opening a run per rule on first use.
    async fn execute_outcomes(
        &self,
        cause: RuleRunCause,
        outcomes: &[MessageOutcome],
        runs: &mut HashMap<RuleId, OpenRun>,
    ) {
        for outcome in outcomes {
            for applied in &outcome.applied {
                if !runs.contains_key(&applied.rule_id) {
                    let (id, name) = (applied.rule_id.clone(), applied.rule_name.clone());
                    match self
                        .inner
                        .store
                        .call(move |s| s.start_rule_run(&id, &name, cause, unix_now()))
                        .await
                    {
                        Ok(run) => {
                            runs.insert(
                                applied.rule_id.clone(),
                                OpenRun {
                                    id: run,
                                    matched: 0,
                                    failure: None,
                                },
                            );
                        }
                        Err(err) => {
                            tracing::warn!(%err, rule = %applied.rule_id, "cannot record a rule run");
                            continue;
                        }
                    }
                }
                let Some(run) = runs.get_mut(&applied.rule_id) else {
                    continue;
                };
                run.matched += 1;
                for planned in &applied.actions {
                    if let Err(err) = self
                        .execute_action(&run.id, &outcome.message_id, planned)
                        .await
                    {
                        tracing::warn!(%err, rule = %applied.rule_id, "a rule action failed");
                        run.failure.get_or_insert_with(|| err.to_string());
                    }
                }
            }
        }
    }

    /// Carries out one planned action and records it in the run.
    async fn execute_action(
        &self,
        run: &RuleRunId,
        message: &MessageId,
        planned: &PlannedAction,
    ) -> Result<()> {
        let (op_id, undo) = match &planned.effect {
            Effect::Queue { op, undo } => {
                let op_ids = self.enqueue(vec![op.clone()]).await?;
                (
                    op_ids.first().and_then(crate::ids::parse_op_id),
                    undo.clone(),
                )
            }
            Effect::Copy { target } => {
                self.copy_message(message, target).await?;
                (None, None)
            }
            Effect::Forward { to, identity_id } => {
                self.rule_forward(message, to, identity_id.clone()).await?;
                (None, None)
            }
            Effect::Redirect { to } => {
                self.rule_redirect(message, to).await?;
                (None, None)
            }
            Effect::Reply {
                template,
                identity_id,
            } => {
                self.rule_reply(message, template, identity_id.clone())
                    .await?;
                (None, None)
            }
            Effect::Notify(_) | Effect::Stop | Effect::Skipped(_) => return Ok(()),
        };
        let action = RuleRunAction {
            message_id: message.clone(),
            op_id,
            action: planned.action.clone(),
            undo,
        };
        let run = run.clone();
        self.inner
            .store
            .call(move |s| s.record_rule_action(&run, &action))
            .await?;
        Ok(())
    }

    /// Finishes the runs and announces them; returns them.
    async fn finish_runs(&self, runs: HashMap<RuleId, OpenRun>) -> Vec<RuleRun> {
        let mut out = Vec::new();
        for (rule_id, run) in runs {
            let state = if run.failure.is_some() {
                RuleRunState::Failed
            } else {
                RuleRunState::Done
            };
            let id = run.id.clone();
            let failure = run.failure.clone();
            let finished = self
                .inner
                .store
                .call(move |s| {
                    s.finish_rule_run(&id, state, run.matched, failure.as_deref(), unix_now())
                })
                .await;
            match finished {
                Ok(record) => {
                    let run = to_run(record);
                    self.broadcast(Event::RuleRunFinished {
                        run: Box::new(run.clone()),
                    });
                    out.push(run);
                }
                Err(err) => tracing::warn!(%err, rule = %rule_id, "cannot finish a rule run"),
            }
        }
        out
    }

    /// `UndoRuleRun`: queues the recorded inverse operations in reverse
    /// order and marks the run undone, in one store transaction. Inverses
    /// that no longer apply (the message is gone, or already back where it
    /// was) are skipped; the rest are still restored. A refused undo
    /// changes nothing.
    async fn undo_rule_run(&self, run_id: &RuleRunId) -> Result<Vec<nail_api::OpId>> {
        let id = run_id.clone();
        let undo = self.inner.store.call(move |s| s.undo_rule_run(&id)).await?;
        if undo.skipped > 0 {
            tracing::info!(run = %run_id, skipped = undo.skipped, "undo skipped actions that no longer apply");
        }
        let op_ids = self.announce_enqueued(&undo.queued);
        self.broadcast(Event::RuleRunFinished {
            run: Box::new(to_run(undo.record)),
        });
        Ok(op_ids)
    }

    /// Runs the arrival rules over the messages waiting in the rule inbox.
    /// Returns how many messages were processed.
    pub(crate) async fn process_rule_inbox(&self) -> Result<usize> {
        let ids = self.inner.store.call(|s| s.rule_inbox(INBOX_BATCH)).await?;
        if ids.is_empty() {
            return Ok(0);
        }
        let set = self.inner.rules.set();
        let has_arrival = set.rules().iter().any(|r| r.runs_on(RuleRunCause::Arrival));
        if has_arrival {
            self.process_messages(&set, RuleRunCause::Arrival, ids.clone())
                .await;
        }
        let done = ids.len();
        self.inner
            .store
            .call(move |s| s.clear_rule_inbox(&ids))
            .await?;
        Ok(done)
    }

    /// Every rule `cause` triggers, in order, on each message.
    async fn process_messages(&self, set: &RuleSet, cause: RuleRunCause, ids: Vec<MessageId>) {
        let loaded = self.load_views(ids).await;
        let (views, mailbox) = match loaded {
            Ok(loaded) => loaded,
            Err(err) => {
                tracing::warn!(%err, "cannot load messages for rules");
                return;
            }
        };
        let outcomes: Vec<MessageOutcome> = {
            let env = Env {
                now: unix_now(),
                utc_offset_secs: utc_offset_secs(),
                mailbox: &mailbox,
            };
            views
                .iter()
                .map(|(data, _)| set.process(cause, data, &env))
                .filter(|o| !o.applied.is_empty())
                .collect()
        };
        if outcomes.is_empty() {
            return;
        }
        let mut runs = HashMap::new();
        self.execute_outcomes(cause, &outcomes, &mut runs).await;
        self.finish_runs(runs).await;
    }

    /// On-send rules on the Sent copy of a message.
    pub(crate) async fn run_on_send_rules(&self, sent_copy: &MessageId) {
        let set = self.inner.rules.set();
        if set.rules().iter().any(|r| r.runs_on(RuleRunCause::OnSend)) {
            self.process_messages(&set, RuleRunCause::OnSend, vec![sent_copy.clone()])
                .await;
        }
    }

    /// Runs the scheduled rules that are due.
    async fn run_due_rules(&self) -> Result<()> {
        let set = self.inner.rules.set();
        if set.rules().is_empty() {
            return Ok(());
        }
        let now = unix_now();
        let states: HashMap<RuleId, RuleState> = {
            let ids: Vec<RuleId> = set.rules().iter().map(|r| r.rule().id.clone()).collect();
            self.inner
                .store
                .call(move |s| {
                    ids.into_iter()
                        .map(|id| Ok((id.clone(), s.rule_state(&id)?)))
                        .collect()
                })
                .await?
        };
        let due: Vec<&CompiledRule> = set.due(now, |id| states.get(id).and_then(|s| s.last_run_at));
        for compiled in due {
            let rule_id = compiled.rule().id.clone();
            let ids = self.candidate_messages(None, compiled.rule()).await?;
            if let Err(err) = self
                .run_one_rule(compiled, RuleRunCause::Schedule, ids)
                .await
            {
                tracing::warn!(%err, rule = %rule_id, "a scheduled rule run failed");
            }
            let mut state = states.get(&rule_id).cloned().unwrap_or_default();
            state.last_run_at = Some(now);
            state.next_run_at = compiled.next_run_at(Some(now), now);
            state.run_count = state.run_count.saturating_add(1);
            self.inner
                .store
                .call(move |s| s.set_rule_state(&rule_id, &state))
                .await?;
        }
        Ok(())
    }
}

/// The rules task: arrival rules for new mail (woken by the sync engines),
/// scheduled rules on every tick.
pub(crate) async fn run_rules(service: Service) {
    let mut events = service.subscribe();
    let tick = service.inner.timing.rule_tick;
    loop {
        loop {
            match service.process_rule_inbox().await {
                Ok(0) => break,
                Ok(_) => {}
                Err(err) => {
                    tracing::warn!(%err, "cannot run the arrival rules");
                    break;
                }
            }
        }
        if let Err(err) = service.run_due_rules().await {
            tracing::warn!(%err, "cannot run the scheduled rules");
        }
        tokio::select! {
            () = tokio::time::sleep(tick) => {}
            () = service.inner.rules_wake.notified() => {}
            event = events.recv() => {
                // New mail is put into the rule inbox before `NewMail` is
                // sent; a lagged receiver simply processes the inbox.
                if let Err(tokio::sync::broadcast::error::RecvError::Closed) = event {
                    return;
                }
            }
        }
    }
}

/// Puts new messages into the rule inbox (called by the sync engines
/// before they announce `NewMail`).
pub(crate) fn queue_for_rules(store: &Store, ids: &[MessageId]) -> nail_store::Result<()> {
    store.queue_for_rules(ids, unix_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_parsed_from_raw_values() {
        assert_eq!(
            parse_addresses("\"Ada L.\" <ada@example.org>, bob@example.org, nonsense"),
            vec![
                Address {
                    name: Some("Ada L.".into()),
                    email: "ada@example.org".into()
                },
                Address {
                    name: None,
                    email: "bob@example.org".into()
                },
            ]
        );
    }

    #[test]
    fn runs_format_times_and_op_ids() {
        let run = to_run(RuleRunRecord {
            id: RuleRunId("run-1".into()),
            rule_id: "r".into(),
            rule_name: "R".into(),
            cause: RuleRunCause::Manual,
            state: RuleRunState::Done,
            started_at: 0,
            finished_at: Some(60),
            matched: 2,
            message: None,
            op_ids: vec![nail_store::OpId(7)],
            undoable: true,
        });
        assert_eq!(run.started_at, "1970-01-01T00:00:00Z");
        assert_eq!(run.finished_at.as_deref(), Some("1970-01-01T00:01:00Z"));
        assert_eq!(run.op_ids, vec![crate::ids::op_id(nail_store::OpId(7))]);
    }

    const RULE: &str = "rule \"Lists\" {\n    when {\n        subject contains=\"news\"\n    }\n    then {\n        flag\n    }\n}\n";

    #[test]
    fn manager_edits_reloads_and_keeps_the_last_valid_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RULES_FILE);
        std::fs::write(&path, format!("// mine\n{RULE}")).unwrap();
        let manager = RulesManager::load(dir.path());
        assert_eq!(manager.rules().len(), 1);
        assert!(manager.errors().is_empty());
        assert_eq!(manager.set().rules().len(), 1);

        manager
            .edit(|doc| doc.remove_rule(&"lists".into()).map_err(|e| rule_error(&e)))
            .unwrap();
        assert!(manager.rules().is_empty());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("rule \"Lists\""), "{text}");
        assert_eq!(manager.reload(), RulesReload::Unchanged);

        std::fs::write(&path, "rule \"x\" {").unwrap();
        assert_eq!(manager.reload(), RulesReload::Invalid);
        assert_eq!(manager.reload(), RulesReload::Unchanged, "reported once");
        assert!(!manager.errors().is_empty());
        let err = manager.edit(|_| Ok(())).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Config);

        std::fs::write(&path, RULE).unwrap();
        assert_eq!(manager.reload(), RulesReload::Changed { was_invalid: true });
        assert_eq!(manager.rules().len(), 1);
    }
}
