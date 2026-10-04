//! Mail hooks on the host side (M4a, ADR 0079; design.md, "Mail hooks").
//!
//! The core calls the hooks of every running plug-in at three points:
//!
//! * [`PluginHost::arrival_hooks`] for each new message (after its own
//!   arrival rules): plug-ins with `hooks.arrival` whose grant reaches the
//!   message's account and whose `filter` matches get the message (and its
//!   body with `mail.read.body`); their actions are applied like a rule's
//!   through the core's write path ([`crate::PluginServices::write`], kind
//!   `organize`; they need `mail.organize` for that account).
//! * [`PluginHost::before_send_hooks`] before a message is queued: vetoes
//!   (named after the plug-in; the user can send anyway), added `X-`
//!   headers and content (HTML sanitized like a message body).
//! * [`PluginHost::open_hooks`] when the reader opens a message: banners
//!   and badges (banner buttons come back as `PluginUiAction` with a
//!   banner target, accepted only for buttons the plug-in showed with that
//!   message).
//!
//! **Budgets.** Every call has the wall-clock budget of its manifest entry
//! (`PluginHookDef::timeout_ms`). A hook that throws, answers something
//! invalid or runs out of time is skipped and counted
//! ([`crate::PluginServices::record_hook_call`]); it never blocks delivery,
//! sending or the reader. A hook that loops is also stopped by the per-turn
//! CPU budget, which stops its plug-in (`cpu-budget`). All plug-ins' hooks
//! run at the same time, each on its own thread.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nail_api::mcp::tools::UpdateMessagesArgs;
use nail_api::plugins::hooks::{
    MAX_ARRIVAL_ACTIONS, PluginArrivalResult, PluginBadge, PluginBanner, PluginBeforeSendResult,
    PluginContentAddition, PluginHeader, PluginMailAction, PluginOpenResult, PluginOutgoingMessage,
    PluginSendVeto,
};
use nail_api::plugins::{
    PluginHookKind, PluginHookOutcome, PluginLogLevel, PluginMessage, PluginPermission,
};
use nail_api::{LocalizedMessage, MessageId, PluginId};
use serde_json::{Value, json};

use crate::host::{Inner, PluginHost, Running, lock};
use crate::l10n;
use crate::runtime::{Invoke, Origin, Registry};
use crate::services::{AccountScope, HookCallRecord, PluginWriteRequest};
use crate::writes::PluginWrite;

/// How one plug-in's hook call went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRun {
    /// The plug-in.
    pub plugin_id: PluginId,
    /// Its name (in the core's language).
    pub plugin_name: String,
    /// How the call ended.
    pub outcome: PluginHookOutcome,
    /// How long it took.
    pub elapsed: Duration,
    /// Why it failed or was skipped (English, for logs).
    pub error: Option<String>,
}

/// What the arrival hooks did with a message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArrivalReport {
    /// One entry per plug-in whose hook was called.
    pub runs: Vec<HookRun>,
    /// The actions handed to the core per plug-in (they run through its
    /// write path; the core may still ask the user per the plug-in's
    /// policy).
    pub actions: Vec<(PluginId, Vec<PluginMailAction>)>,
}

/// What the before-send hooks answered for a message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BeforeSendReport {
    /// One entry per plug-in whose hook was called.
    pub runs: Vec<HookRun>,
    /// Vetoes (each names its plug-in); empty = send.
    pub vetoes: Vec<PluginSendVeto>,
    /// Headers to add (validated `X-` headers), by plug-in.
    pub headers: Vec<(PluginId, PluginHeader)>,
    /// Content to add (HTML sanitized), by plug-in.
    pub additions: Vec<(PluginId, PluginContentAddition)>,
}

impl BeforeSendReport {
    /// Whether a plug-in vetoed.
    pub fn vetoed(&self) -> bool {
        !self.vetoes.is_empty()
    }
}

/// What the open hooks show with a message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// One entry per plug-in whose hook was called.
    pub runs: Vec<HookRun>,
    /// Banners.
    pub banners: Vec<PluginBanner>,
    /// Badges.
    pub badges: Vec<PluginBadge>,
}

/// Calls one plug-in's hook and waits at most its budget.
async fn call_hook(
    inner: &Arc<Inner>,
    plugin: &PluginId,
    running: &Running,
    kind: PluginHookKind,
    input: Value,
    origin: Origin,
) -> (HookRun, Option<Value>) {
    let budget = running.manifest.hook(kind).map_or(
        kind.default_timeout_ms(),
        nail_api::plugins::PluginHookDef::timeout_ms,
    );
    let budget = Duration::from_millis(u64::from(budget));
    let started = Instant::now();
    let mut call = Invoke::new(Registry::Hook, kind.as_str(), input);
    call.origin = origin;
    let outcome = match inner.invoke(plugin, Some(running.generation), call) {
        Err(e) => Err((PluginHookOutcome::Failed, e.message)),
        Ok(rx) => match tokio::time::timeout(budget, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(e))) => Err((PluginHookOutcome::Failed, e.message)),
            Ok(Err(_)) => Err((PluginHookOutcome::Failed, "the plug-in stopped".to_owned())),
            Err(_) => Err((
                PluginHookOutcome::TimedOut,
                format!("did not answer within {} ms", budget.as_millis()),
            )),
        },
    };
    let elapsed = started.elapsed();
    let run = |outcome, error| HookRun {
        plugin_id: plugin.clone(),
        plugin_name: running.name.clone(),
        outcome,
        elapsed,
        error,
    };
    match outcome {
        Ok(value) => (run(PluginHookOutcome::Ok, None), Some(value)),
        Err((outcome, error)) => (run(outcome, Some(error)), None),
    }
}

/// Parses a hook's answer (`undefined` / `null` = nothing).
fn answer<T: serde::de::DeserializeOwned + Default>(value: Value) -> Result<T, String> {
    if value.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(value).map_err(|e| format!("invalid answer: {e}"))
}

impl Inner {
    /// Records a call, logs a failure (at most once a minute per plug-in
    /// and hook).
    fn finish_hook(self: &Arc<Self>, kind: PluginHookKind, run: &HookRun) {
        let services = Arc::clone(&self.services);
        let record = HookCallRecord {
            plugin_id: run.plugin_id.clone(),
            hook: kind,
            at: services.now().timestamp(),
            ms: u64::try_from(run.elapsed.as_millis()).unwrap_or(u64::MAX),
            outcome: run.outcome,
            error: run.error.clone(),
        };
        self.handle.spawn(async move {
            drop(services.record_hook_call(record).await);
        });
        if !matches!(
            run.outcome,
            PluginHookOutcome::Failed | PluginHookOutcome::TimedOut
        ) {
            return;
        }
        let due = {
            let mut logs = lock(&self.hook_logs);
            let key = (run.plugin_id.clone(), kind);
            let due = logs
                .get(&key)
                .is_none_or(|t| t.elapsed() >= Duration::from_mins(1));
            if due {
                logs.insert(key, Instant::now());
            }
            due
        };
        if !due {
            return;
        }
        let hook_name = kind.message_id();
        let detail = run.error.clone().unwrap_or_default();
        let localized = if run.outcome == PluginHookOutcome::TimedOut {
            let ms = self
                .running_info(&run.plugin_id)
                .ok()
                .and_then(|r| {
                    r.manifest
                        .hook(kind)
                        .map(nail_api::plugins::PluginHookDef::timeout_ms)
                })
                .unwrap_or_else(|| kind.default_timeout_ms());
            LocalizedMessage::new(l10n::HOOK_TIMED_OUT)
                .arg("plugin", run.plugin_name.as_str())
                .arg("hook", hook_name)
                .arg("ms", u64::from(ms))
        } else {
            LocalizedMessage::new(l10n::HOOK_FAILED)
                .arg("plugin", run.plugin_name.as_str())
                .arg("hook", hook_name)
                .arg("detail", detail.as_str())
        };
        self.host_log(
            &run.plugin_id,
            PluginLogLevel::Warn,
            format!("{} hook skipped: {detail}", kind.as_str()),
            localized,
        );
    }

    /// Whether a hook applies to a message: its grant (with `mail.read`)
    /// reaches the account and its filter matches.
    async fn hook_applies(
        &self,
        running: &Running,
        kind: PluginHookKind,
        message: &PluginMessage,
    ) -> bool {
        let Some(def) = running.manifest.hook(kind) else {
            return false;
        };
        let expand = self.expand();
        let Ok(scope) = running
            .gate
            .scope_with_read(&kind.permission(), kind.as_str(), &expand)
        else {
            return false;
        };
        if !scope.contains(&message.account_id) {
            return false;
        }
        match &def.filter {
            None => true,
            Some(filter) => self
                .services
                .message_matches(message.id.clone(), filter.clone())
                .await
                .unwrap_or(false),
        }
    }

    /// The hook input for a message: the message, and its body when the
    /// plug-in may read it (`mail.read.body`, never end-to-end encrypted).
    async fn message_input(&self, running: &Running, message: &PluginMessage) -> Value {
        let mut input = json!({ "message": message });
        let body_ok = running
            .gate
            .body_scope("hooks", &self.expand())
            .is_ok_and(|s| s.contains(&message.account_id));
        if body_ok
            && !message.encrypted
            && let Ok(Some(body)) = self.services.mail_body(message.id.clone()).await
        {
            input["body"] = serde_json::to_value(body).unwrap_or_default();
        }
        input
    }
}

/// One `UpdateMessagesArgs` from an arrival hook's actions (later actions
/// win over earlier ones of the same kind).
fn arrival_write(message: &MessageId, actions: &[PluginMailAction]) -> UpdateMessagesArgs {
    let mut args = UpdateMessagesArgs {
        message_ids: vec![message.to_string()],
        ..UpdateMessagesArgs::default()
    };
    for action in actions {
        match action {
            PluginMailAction::AddTag { tag } => {
                args.remove_tags.retain(|t| t != tag);
                if !args.add_tags.contains(tag) {
                    args.add_tags.push(tag.clone());
                }
            }
            PluginMailAction::RemoveTag { tag } => {
                args.add_tags.retain(|t| t != tag);
                if !args.remove_tags.contains(tag) {
                    args.remove_tags.push(tag.clone());
                }
            }
            PluginMailAction::SetFlagged { flagged } => args.flagged = Some(*flagged),
            PluginMailAction::SetRead { read } => args.read = Some(*read),
            PluginMailAction::Move { folder_id } => {
                args.move_to_folder_id = Some(folder_id.to_string());
                args.archive = false;
            }
            PluginMailAction::Archive => {
                args.archive = true;
                args.move_to_folder_id = None;
            }
        }
    }
    args
}

fn arrival_problems(result: &PluginArrivalResult) -> Option<String> {
    if result.actions.len() > MAX_ARRIVAL_ACTIONS {
        return Some("too many actions".to_owned());
    }
    let bad_tag = |t: &str| t.trim().is_empty() || t.chars().count() > 100;
    let bad = result.actions.iter().any(|a| match a {
        PluginMailAction::AddTag { tag } | PluginMailAction::RemoveTag { tag } => bad_tag(tag),
        PluginMailAction::Move { folder_id } => folder_id.as_str().is_empty(),
        _ => false,
    });
    bad.then(|| "invalid action".to_owned())
}

impl PluginHost {
    /// Runs the arrival hooks for a new message (see the module docs) and
    /// hands their actions to the core. Never fails: hooks that fail or
    /// time out are skipped and counted.
    pub async fn arrival_hooks(&self, message_id: &MessageId) -> ArrivalReport {
        let inner = Arc::clone(self.inner());
        let Ok(Some(message)) = inner.services.mail_get(message_id.clone()).await else {
            return ArrivalReport::default();
        };
        let mut tasks = tokio::task::JoinSet::new();
        for (plugin, running) in inner.running_all() {
            let inner = Arc::clone(&inner);
            let message = message.clone();
            tasks.spawn(async move {
                let kind = PluginHookKind::Arrival;
                if !inner.hook_applies(&running, kind, &message).await {
                    return None;
                }
                let input = inner.message_input(&running, &message).await;
                let (mut run, value) =
                    call_hook(&inner, &plugin, &running, kind, input, Origin::default()).await;
                let mut actions = Vec::new();
                if let Some(value) = value {
                    match answer::<PluginArrivalResult>(value) {
                        Ok(result) => match arrival_problems(&result) {
                            None => actions = result.actions,
                            Some(problem) => {
                                run.outcome = PluginHookOutcome::Failed;
                                run.error = Some(problem);
                            }
                        },
                        Err(problem) => {
                            run.outcome = PluginHookOutcome::Failed;
                            run.error = Some(problem);
                        }
                    }
                }
                inner.finish_hook(kind, &run);
                let submitted = if actions.is_empty() {
                    None
                } else {
                    submit_arrival(&inner, &plugin, &running, &message, actions)
                };
                Some((run, submitted))
            });
        }
        let mut report = ArrivalReport::default();
        while let Some(done) = tasks.join_next().await {
            if let Ok(Some((run, submitted))) = done {
                if let Some(actions) = submitted {
                    report.actions.push((run.plugin_id.clone(), actions));
                }
                report.runs.push(run);
            }
        }
        report.runs.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        report.actions.sort_by(|a, b| a.0.cmp(&b.0));
        report
    }

    /// Runs the before-send hooks for an outgoing message (see the module
    /// docs). Never fails.
    pub async fn before_send_hooks(&self, message: &PluginOutgoingMessage) -> BeforeSendReport {
        let inner = Arc::clone(self.inner());
        let input = serde_json::to_value(before_send_view(message)).unwrap_or_default();
        let mut tasks = tokio::task::JoinSet::new();
        for (plugin, running) in inner.running_all() {
            if running.manifest.hook(PluginHookKind::BeforeSend).is_none() {
                continue;
            }
            let reaches = running
                .gate
                .scope(
                    &PluginPermission::HooksBeforeSend,
                    "hooks.beforeSend",
                    &inner.expand(),
                )
                .is_ok_and(|s| s.contains(&message.account_id));
            if !reaches {
                continue;
            }
            let inner = Arc::clone(&inner);
            let input = input.clone();
            tasks.spawn(async move {
                let kind = PluginHookKind::BeforeSend;
                let (mut run, value) =
                    call_hook(&inner, &plugin, &running, kind, input, Origin::default()).await;
                let mut result = None;
                if let Some(value) = value {
                    match answer::<PluginBeforeSendResult>(value) {
                        Ok(answer) if answer.problems().is_empty() => {
                            if answer.veto.is_some() {
                                run.outcome = PluginHookOutcome::Vetoed;
                            }
                            result = Some(answer);
                        }
                        Ok(answer) => {
                            run.outcome = PluginHookOutcome::Failed;
                            run.error = Some(answer.problems().join(", "));
                        }
                        Err(problem) => {
                            run.outcome = PluginHookOutcome::Failed;
                            run.error = Some(problem);
                        }
                    }
                }
                inner.finish_hook(kind, &run);
                (run, result)
            });
        }
        let mut report = BeforeSendReport::default();
        let mut answers = Vec::new();
        while let Some(done) = tasks.join_next().await {
            if let Ok((run, result)) = done {
                if let Some(result) = result {
                    answers.push((run.plugin_id.clone(), run.plugin_name.clone(), result));
                }
                report.runs.push(run);
            }
        }
        report.runs.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        answers.sort_by(|a, b| a.0.cmp(&b.0));
        for (plugin, name, result) in answers {
            if let Some(veto) = result.veto {
                report.vetoes.push(PluginSendVeto {
                    plugin_id: plugin.clone(),
                    plugin_name: name,
                    reason: veto.reason,
                });
            }
            for header in result.headers {
                report.headers.push((plugin.clone(), header));
            }
            for mut addition in result.add_content {
                addition.html = addition
                    .html
                    .map(|html| nail_mime::sanitize_html(&html).html);
                report.additions.push((plugin.clone(), addition));
            }
        }
        report
    }

    /// Runs the open hooks for a message the reader opens
    /// (`GetMessagePluginAnnotations`; see the module docs). Never fails.
    pub async fn open_hooks(
        &self,
        message_id: &MessageId,
        client_id: Option<String>,
    ) -> OpenReport {
        let inner = Arc::clone(self.inner());
        let Ok(Some(message)) = inner.services.mail_get(message_id.clone()).await else {
            return OpenReport::default();
        };
        let mut tasks = tokio::task::JoinSet::new();
        for (plugin, running) in inner.running_all() {
            let inner = Arc::clone(&inner);
            let message = message.clone();
            let origin = Origin::client(client_id.clone());
            tasks.spawn(async move {
                let kind = PluginHookKind::Open;
                if !inner.hook_applies(&running, kind, &message).await {
                    return None;
                }
                let input = inner.message_input(&running, &message).await;
                let (mut run, value) =
                    call_hook(&inner, &plugin, &running, kind, input, origin).await;
                let mut result = PluginOpenResult::default();
                if let Some(value) = value {
                    match answer::<PluginOpenResult>(value) {
                        Ok(answer) if answer.problems().is_empty() => result = answer,
                        Ok(answer) => {
                            run.outcome = PluginHookOutcome::Failed;
                            run.error = Some(answer.problems().join(", "));
                        }
                        Err(problem) => {
                            run.outcome = PluginHookOutcome::Failed;
                            run.error = Some(problem);
                        }
                    }
                }
                inner.finish_hook(kind, &run);
                let shown: HashMap<String, Vec<String>> = result
                    .banners
                    .iter()
                    .map(|b| {
                        (
                            b.id.clone(),
                            b.buttons.iter().map(|x| x.id.clone()).collect(),
                        )
                    })
                    .collect();
                inner.remember_banners(&plugin, &message.id, shown);
                Some((run, result))
            });
        }
        let mut found = Vec::new();
        while let Some(done) = tasks.join_next().await {
            if let Ok(Some(entry)) = done {
                found.push(entry);
            }
        }
        found.sort_by(|a, b| a.0.plugin_id.cmp(&b.0.plugin_id));
        let mut report = OpenReport::default();
        for (run, result) in found {
            for banner in result.banners {
                report.banners.push(PluginBanner {
                    plugin_id: run.plugin_id.clone(),
                    plugin_name: run.plugin_name.clone(),
                    banner,
                });
            }
            for badge in result.badges {
                report.badges.push(PluginBadge {
                    plugin_id: run.plugin_id.clone(),
                    plugin_name: run.plugin_name.clone(),
                    badge,
                });
            }
            report.runs.push(run);
        }
        report
    }
}

/// What before-send hooks see: never the content of a message that will be
/// end-to-end encrypted (`mail.read.encrypted` is not offered; review
/// M4a) — its text is empty and its HTML absent. Recipients, subject and
/// attachment names stay (they decide routing and checks).
pub fn before_send_view(message: &PluginOutgoingMessage) -> PluginOutgoingMessage {
    let mut view = message.clone();
    if view.encrypted {
        view.text = String::new();
        view.html = None;
    }
    view
}

/// Hands an arrival hook's actions to the core (not awaited: the user may
/// have to confirm them, and delivery never waits). `None` when the plug-in
/// may not organize mail in the message's account.
fn submit_arrival(
    inner: &Arc<Inner>,
    plugin: &PluginId,
    running: &Running,
    message: &PluginMessage,
    actions: Vec<PluginMailAction>,
) -> Option<Vec<PluginMailAction>> {
    let allowed = running
        .gate
        .scope_with_read(
            &PluginPermission::MailOrganize,
            "hooks.arrival",
            &inner.expand(),
        )
        .is_ok_and(|s| s.contains(&message.account_id));
    if !allowed {
        inner.host_log(
            plugin,
            PluginLogLevel::Warn,
            "arrival hook actions dropped: no mail.organize for the account",
            LocalizedMessage::new(l10n::HOOK_ACTIONS_DENIED).arg("plugin", running.name.as_str()),
        );
        return None;
    }
    let request = PluginWriteRequest {
        attribution: nail_api::agents::ActionAttribution {
            actor: nail_api::agents::ActionActor::plugin(plugin.clone()),
            name: running.name.clone(),
            on_behalf_of: None,
        },
        kind: nail_api::agents::ActionKind::Organize,
        operation: "hooks.arrival".to_owned(),
        write: PluginWrite::UpdateMessages(arrival_write(&message.id, &actions)),
        // The checked account only.
        scope: AccountScope::Only(vec![message.account_id.clone()]),
    };
    let services = Arc::clone(&inner.services);
    let inner2 = Arc::clone(inner);
    let plugin2 = plugin.clone();
    inner.handle.spawn(async move {
        if let Err(e) = services.write(request).await {
            inner2.host_log(
                &plugin2,
                PluginLogLevel::Warn,
                format!("arrival hook actions failed: {}", e.message),
                LocalizedMessage::new(l10n::HOOK_FAILED)
                    .arg("plugin", plugin2.as_str())
                    .arg("hook", PluginHookKind::Arrival.message_id())
                    .arg("detail", e.message.as_str()),
            );
        }
    });
    Some(actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn arrival_actions_become_one_update() {
        let actions = vec![
            PluginMailAction::AddTag { tag: "crm".into() },
            PluginMailAction::AddTag { tag: "old".into() },
            PluginMailAction::RemoveTag { tag: "old".into() },
            PluginMailAction::SetFlagged { flagged: true },
            PluginMailAction::SetRead { read: false },
            PluginMailAction::Move {
                folder_id: "f1".into(),
            },
            PluginMailAction::Archive,
        ];
        let args = arrival_write(&"m1".into(), &actions);
        assert_eq!(args.message_ids, ["m1"]);
        assert_eq!(args.add_tags, ["crm"]);
        assert_eq!(args.remove_tags, ["old"]);
        assert_eq!(args.flagged, Some(true));
        assert_eq!(args.read, Some(false));
        assert!(args.archive);
        assert_eq!(args.move_to_folder_id, None);
        let moved = arrival_write(
            &"m1".into(),
            &[
                PluginMailAction::Archive,
                PluginMailAction::Move {
                    folder_id: "f2".into(),
                },
            ],
        );
        assert!(!moved.archive);
        assert_eq!(moved.move_to_folder_id.as_deref(), Some("f2"));
    }

    #[test]
    fn arrival_answers_are_checked() {
        let ok: PluginArrivalResult =
            answer(json!({"actions": [{"type": "addTag", "tag": "x"}]})).unwrap();
        assert_eq!(arrival_problems(&ok), None);
        let nothing: PluginArrivalResult = answer(Value::Null).unwrap();
        assert!(nothing.actions.is_empty());
        let empty_tag: PluginArrivalResult =
            answer(json!({"actions": [{"type": "addTag", "tag": " "}]})).unwrap();
        assert!(arrival_problems(&empty_tag).is_some());
        let many = PluginArrivalResult {
            actions: vec![PluginMailAction::Archive; MAX_ARRIVAL_ACTIONS + 1],
        };
        assert!(arrival_problems(&many).is_some());
        assert!(answer::<PluginArrivalResult>(json!({"actions": [{"type": "delete"}]})).is_err());
    }
}
