//! Rules in order: triggers, scope, conditions, actions; dry runs and
//! schedules.

use nail_api::rules::{Rule, RuleAction, RuleRunCause, RuleScope, RuleTrigger};
use nail_api::{LocalizedMessage, MessageId, RuleId};
use serde::{Deserialize, Serialize};

use crate::condition::{CompiledCondition, ConditionError, ConditionHit};
use crate::message::{Env, MessageView};
use crate::plan::{Flow, MessageState, PlannedAction, plan_actions};

/// Where a problem of a rule is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum IssueLocation {
    /// The rule as a whole (name, id).
    Rule,
    /// A trigger, by index.
    Trigger {
        /// Index in `triggers`.
        index: u32,
    },
    /// A condition, by path.
    Condition {
        /// Child indexes from the root.
        path: Vec<u32>,
    },
    /// An action, by index.
    Action {
        /// Index in `actions`.
        index: u32,
    },
}

/// A problem that keeps a rule from being saved or run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleIssue {
    /// Where.
    pub location: IssueLocation,
    /// Diagnostic (English).
    pub message: String,
    /// The user-facing text: a `rule-issue-*` id of
    /// `locales/<lang>/core.ftl` whose arguments name the trigger, action
    /// or condition (`trigger`, `action`: 1-based; `condition`: a path like
    /// `2.1` or `root`).
    pub localized: LocalizedMessage,
}

impl std::fmt::Display for RuleIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.location {
            IssueLocation::Rule => write!(f, "{}", self.message),
            IssueLocation::Trigger { index } => {
                write!(f, "trigger {}: {}", index + 1, self.message)
            }
            IssueLocation::Condition { path } => write!(
                f,
                "{}",
                ConditionError {
                    path: path.clone(),
                    message: self.message.clone(),
                    localized: self.localized.clone(),
                }
            ),
            IssueLocation::Action { index } => write!(f, "action {}: {}", index + 1, self.message),
        }
    }
}

/// Every problem of a rule: invalid regular expressions, queries and
/// dates, empty names, tags and recipient lists, zero intervals.
pub fn validate(rule: &Rule) -> Vec<RuleIssue> {
    check(rule).0
}

/// [`validate`] plus the compiled condition tree (`None` when it has
/// issues).
fn check(rule: &Rule) -> (Vec<RuleIssue>, Option<CompiledCondition>) {
    let mut issues = Vec::new();
    let mut push = |location: IssueLocation, message: &str, id: &str| {
        let localized = match &location {
            IssueLocation::Rule => LocalizedMessage::new(id),
            IssueLocation::Trigger { index } => {
                LocalizedMessage::new(id).arg("trigger", index.saturating_add(1))
            }
            IssueLocation::Action { index } => {
                LocalizedMessage::new(id).arg("action", index.saturating_add(1))
            }
            IssueLocation::Condition { path } => {
                LocalizedMessage::new(id).arg("condition", crate::condition::condition_label(path))
            }
        };
        issues.push(RuleIssue {
            location,
            message: message.to_owned(),
            localized,
        });
    };
    if rule.name.trim().is_empty() {
        push(
            IssueLocation::Rule,
            "the rule needs a name",
            "rule-issue-name-empty",
        );
    }
    for (i, trigger) in rule.triggers.iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        match trigger {
            RuleTrigger::Schedule {
                interval_minutes: 0,
            } => push(
                IssueLocation::Trigger { index },
                "the schedule interval must be at least one minute",
                "rule-issue-schedule-zero",
            ),
            RuleTrigger::OlderThan { days: 0 } => push(
                IssueLocation::Trigger { index },
                "the age must be at least one day",
                "rule-issue-age-zero",
            ),
            _ => {}
        }
    }
    for (i, action) in rule.actions.iter().enumerate() {
        let location = IssueLocation::Action {
            index: u32::try_from(i).unwrap_or(u32::MAX),
        };
        match action {
            RuleAction::Tag { tag } | RuleAction::Untag { tag } if tag.trim().is_empty() => {
                push(location, "the tag is empty", "rule-issue-tag-empty");
            }
            RuleAction::Forward { to, .. } | RuleAction::Redirect { to } if to.is_empty() => {
                push(
                    location,
                    "there are no recipients",
                    "rule-issue-no-recipients",
                );
            }
            RuleAction::Forward { to, .. } | RuleAction::Redirect { to }
                if to.iter().any(|a| !a.contains('@')) =>
            {
                push(
                    location,
                    "a recipient is not an email address",
                    "rule-issue-recipient-invalid",
                );
            }
            RuleAction::Reply { template, .. } if template.trim().is_empty() => {
                push(
                    location,
                    "the reply template is empty",
                    "rule-issue-template-empty",
                );
            }
            RuleAction::Move { folder } | RuleAction::Copy { folder } => {
                if let nail_api::rules::FolderRef::Path { path, .. } = folder
                    && path.trim().is_empty()
                {
                    push(location, "the folder is empty", "rule-issue-folder-empty");
                }
            }
            _ => {}
        }
    }
    let condition = match CompiledCondition::new(&rule.conditions) {
        Ok(condition) => Some(condition),
        Err(errors) => {
            issues.extend(errors.into_iter().map(|e| RuleIssue {
                location: IssueLocation::Condition { path: e.path },
                message: e.message,
                localized: e.localized,
            }));
            None
        }
    };
    (issues, condition)
}

/// A rule ready to be evaluated.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    rule: Rule,
    condition: CompiledCondition,
}

/// The result of evaluating one rule against one message, with an
/// explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    /// The rule matched (scope, age requirement and conditions).
    pub matched: bool,
    /// The message is in the rule's scope.
    pub in_scope: bool,
    /// Every evaluated condition (only when in scope).
    pub hits: Vec<ConditionHit>,
}

impl CompiledRule {
    /// Compiles a rule; fails with every problem [`validate`] finds.
    pub fn new(rule: Rule) -> Result<Self, Vec<RuleIssue>> {
        match check(&rule) {
            (issues, Some(condition)) if issues.is_empty() => Ok(Self { rule, condition }),
            (issues, _) => Err(issues),
        }
    }

    /// The rule.
    pub fn rule(&self) -> &Rule {
        &self.rule
    }

    /// Whether the rule runs for `cause`: enabled rules run automatically
    /// when they have a matching trigger (`schedule` covers both
    /// `schedule` and `older-than`); any rule, even a disabled one, can be
    /// run by hand.
    pub fn runs_on(&self, cause: RuleRunCause) -> bool {
        if cause == RuleRunCause::Manual {
            return true;
        }
        self.rule.enabled
            && self.rule.triggers.iter().any(|t| {
                matches!(
                    (t, cause),
                    (RuleTrigger::Arrival, RuleRunCause::Arrival)
                        | (RuleTrigger::OnSend, RuleRunCause::OnSend)
                        | (
                            RuleTrigger::Schedule { .. } | RuleTrigger::OlderThan { .. },
                            RuleRunCause::Schedule
                        )
                )
            })
    }

    /// Whether the message is in the rule's scope (an empty scope covers
    /// everything; otherwise any one scope entry must cover it).
    pub fn in_scope(&self, message: &dyn MessageView, env: &Env<'_>) -> bool {
        self.rule.scope.is_empty()
            || self.rule.scope.iter().any(|scope| match scope {
                RuleScope::Account { account_id } => {
                    env.account_matches(message.account_id(), account_id)
                }
                RuleScope::Folder { folder } => env
                    .resolve_folder(folder, message.account_id())
                    .is_some_and(|f| &f.id == message.folder_id()),
            })
    }

    /// The age an `older-than` trigger requires for `cause`: always for
    /// scheduled runs, and for other runs when the rule has no arrival or
    /// on-send trigger (then `older-than` is what the rule is about).
    fn age_requirement(&self, cause: Option<RuleRunCause>) -> Option<u32> {
        let days = self.rule.triggers.iter().find_map(|t| match t {
            RuleTrigger::OlderThan { days } => Some(*days),
            _ => None,
        })?;
        let event_driven = self
            .rule
            .triggers
            .iter()
            .any(|t| matches!(t, RuleTrigger::Arrival | RuleTrigger::OnSend));
        match cause {
            Some(RuleRunCause::Schedule) => Some(days),
            Some(RuleRunCause::Arrival | RuleRunCause::OnSend) => None,
            Some(RuleRunCause::Manual) | None => (!event_driven).then_some(days),
        }
    }

    fn old_enough(
        &self,
        message: &dyn MessageView,
        env: &Env<'_>,
        cause: Option<RuleRunCause>,
    ) -> bool {
        self.age_requirement(cause)
            .is_none_or(|days| env.now - message.date() > i64::from(days) * 86_400)
    }

    /// Whether the rule matches the message (scope, `older-than` age and
    /// conditions). `cause` `None` is a dry run.
    pub fn matches(
        &self,
        message: &dyn MessageView,
        env: &Env<'_>,
        cause: Option<RuleRunCause>,
    ) -> bool {
        self.in_scope(message, env)
            && self.old_enough(message, env, cause)
            && self.condition.matches(message, env)
    }

    /// Like [`Self::matches`], explaining which conditions matched.
    pub fn explain(
        &self,
        message: &dyn MessageView,
        env: &Env<'_>,
        cause: Option<RuleRunCause>,
    ) -> Explanation {
        if !self.in_scope(message, env) {
            return Explanation {
                matched: false,
                in_scope: false,
                hits: Vec::new(),
            };
        }
        let (matched, hits) = self.condition.explain(message, env);
        Explanation {
            matched: matched && self.old_enough(message, env, cause),
            in_scope: true,
            hits,
        }
    }

    /// Plans the rule's actions for a message in `state`.
    pub fn plan(
        &self,
        message: &dyn MessageView,
        state: &mut MessageState,
        env: &Env<'_>,
    ) -> (Vec<PlannedAction>, Flow) {
        plan_actions(&self.rule.actions, state, message, env)
    }

    /// When the rule should next run on its own (schedule and `older-than`
    /// triggers; `older-than` is checked hourly), given its last scheduled
    /// run. `None`: never (disabled, or no such trigger).
    pub fn next_run_at(&self, last_run_at: Option<i64>, now: i64) -> Option<i64> {
        if !self.rule.enabled {
            return None;
        }
        self.rule
            .triggers
            .iter()
            .filter_map(|t| match t {
                RuleTrigger::Schedule { interval_minutes } => {
                    Some(i64::from(*interval_minutes) * 60)
                }
                RuleTrigger::OlderThan { .. } => Some(3_600),
                _ => None,
            })
            .map(|every| last_run_at.map_or(now, |last| last + every))
            .min()
    }

    /// Whether a scheduled run is due now.
    pub fn is_due(&self, last_run_at: Option<i64>, now: i64) -> bool {
        self.next_run_at(last_run_at, now)
            .is_some_and(|at| at <= now)
    }
}

/// What one rule did to one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedRule {
    /// The rule.
    pub rule_id: RuleId,
    /// Its name.
    pub rule_name: String,
    /// Its actions for this message (up to a `stop`).
    pub actions: Vec<PlannedAction>,
}

/// Everything the rules planned for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageOutcome {
    /// The message.
    pub message_id: MessageId,
    /// The rules that matched, in order.
    pub applied: Vec<AppliedRule>,
    /// The last notify choice of the rules, if any.
    pub notify: Option<bool>,
    /// A rule stopped processing (a `stop`, or a delete).
    pub stopped: bool,
    /// The message after all planned actions.
    pub state: MessageState,
}

impl MessageOutcome {
    /// Whether to show the new-mail notification, given the default.
    pub fn notify_or(&self, default: bool) -> bool {
        self.notify.unwrap_or(default)
    }

    /// Every planned action with its rule, in order.
    pub fn actions(&self) -> impl Iterator<Item = (&AppliedRule, &PlannedAction)> {
        self.applied
            .iter()
            .flat_map(|r| r.actions.iter().map(move |a| (r, a)))
    }
}

/// The rules of `rules.kdl`, compiled, in file order.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    rules: Vec<CompiledRule>,
    broken: Vec<(RuleId, Vec<RuleIssue>)>,
}

impl RuleSet {
    /// Compiles the rules. A broken rule is left out (see
    /// [`Self::broken`]); the others still run.
    pub fn new(rules: impl IntoIterator<Item = Rule>) -> Self {
        let mut set = Self::default();
        for rule in rules {
            let id = rule.id.clone();
            match CompiledRule::new(rule) {
                Ok(compiled) => set.rules.push(compiled),
                Err(issues) => set.broken.push((id, issues)),
            }
        }
        set
    }

    /// The compiled rules, in order.
    pub fn rules(&self) -> &[CompiledRule] {
        &self.rules
    }

    /// Rules left out, with their problems.
    pub fn broken(&self) -> &[(RuleId, Vec<RuleIssue>)] {
        &self.broken
    }

    /// A rule by id.
    pub fn get(&self, id: &RuleId) -> Option<&CompiledRule> {
        self.rules.iter().find(|r| &r.rule.id == id)
    }

    /// Runs every rule that `cause` triggers on a message, in order: each
    /// matching rule plans its actions on the state the earlier rules left;
    /// a `stop` (or a delete) ends processing.
    pub fn process(
        &self,
        cause: RuleRunCause,
        message: &dyn MessageView,
        env: &Env<'_>,
    ) -> MessageOutcome {
        run(
            self.rules.iter().filter(|r| r.runs_on(cause)),
            Some(cause),
            message,
            env,
        )
    }

    /// The rules whose scheduled run is due, given each one's last run.
    pub fn due(
        &self,
        now: i64,
        last_run_at: impl Fn(&RuleId) -> Option<i64>,
    ) -> Vec<&CompiledRule> {
        self.rules
            .iter()
            .filter(|r| r.is_due(last_run_at(&r.rule.id), now))
            .collect()
    }
}

/// A message as earlier rules left it.
struct WithState<'m> {
    inner: &'m dyn MessageView,
    state: MessageState,
}

impl MessageView for WithState<'_> {
    fn id(&self) -> &MessageId {
        self.inner.id()
    }
    fn account_id(&self) -> &nail_api::AccountId {
        &self.state.account_id
    }
    fn folder_id(&self) -> &nail_api::FolderId {
        &self.state.folder_id
    }
    fn header_values(&self, name: &str) -> Vec<String> {
        self.inner.header_values(name)
    }
    fn addresses(&self, field: nail_api::rules::AddressField) -> Vec<nail_api::Address> {
        self.inner.addresses(field)
    }
    fn subject(&self) -> String {
        self.inner.subject()
    }
    fn body_text(&self) -> Option<String> {
        self.inner.body_text()
    }
    fn attachments(&self) -> Vec<crate::message::Attachment> {
        self.inner.attachments()
    }
    fn size(&self) -> u64 {
        self.inner.size()
    }
    fn date(&self) -> i64 {
        self.inner.date()
    }
    fn flags(&self) -> nail_api::Flags {
        self.state.flags
    }
    fn tags(&self) -> Vec<String> {
        self.state.tags.clone()
    }
    fn gmail_category(&self) -> Option<String> {
        self.inner.gmail_category()
    }
    fn exchange_focus(&self) -> Option<String> {
        self.inner.exchange_focus()
    }
}

fn run<'r>(
    rules: impl Iterator<Item = &'r CompiledRule>,
    cause: Option<RuleRunCause>,
    message: &dyn MessageView,
    env: &Env<'_>,
) -> MessageOutcome {
    let mut state = MessageState::of(message);
    let mut outcome = MessageOutcome {
        message_id: message.id().clone(),
        applied: Vec::new(),
        notify: None,
        stopped: false,
        state: state.clone(),
    };
    for rule in rules {
        // Later rules see what earlier ones did (folder, flags, tags).
        let seen = WithState {
            inner: message,
            state: state.clone(),
        };
        if !rule.matches(&seen, env, cause) {
            continue;
        }
        let (actions, flow) = rule.plan(&seen, &mut state, env);
        if flow.notify.is_some() {
            outcome.notify = flow.notify;
        }
        outcome.applied.push(AppliedRule {
            rule_id: rule.rule.id.clone(),
            rule_name: rule.rule.name.clone(),
            actions,
        });
        if flow.stopped {
            outcome.stopped = true;
            break;
        }
    }
    outcome.state = state;
    outcome
}

/// Runs one rule on the given messages (a manual run, or one scheduled
/// run of that rule). Only matching messages are returned.
pub fn run_rule<'m>(
    rule: &CompiledRule,
    cause: RuleRunCause,
    messages: impl IntoIterator<Item = &'m dyn MessageView>,
    env: &Env<'_>,
) -> Vec<MessageOutcome> {
    messages
        .into_iter()
        .map(|m| run(std::iter::once(rule), Some(cause), m, env))
        .filter(|o| !o.applied.is_empty())
        .collect()
}

/// A message a dry run matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestMatch {
    /// The message.
    pub message_id: MessageId,
    /// The actions that would run, in order, up to a `stop`
    /// ([`nail_api::rules::RuleMatch::actions`]).
    pub actions: Vec<RuleAction>,
    /// What each of them would do.
    pub planned: Vec<PlannedAction>,
    /// Which conditions matched and on what.
    pub hits: Vec<ConditionHit>,
}

/// The result of a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TestReport {
    /// Messages examined.
    pub scanned: u32,
    /// Matches, in the order the messages were given, up to the limit.
    pub matches: Vec<TestMatch>,
}

/// Dry run ("test rule"): which of `messages` the rule matches, what it
/// would do and why it matched. Nothing is changed. Stops after `limit`
/// matches.
pub fn test_rule<'m>(
    rule: &CompiledRule,
    messages: impl IntoIterator<Item = &'m dyn MessageView>,
    env: &Env<'_>,
    limit: usize,
) -> TestReport {
    let mut report = TestReport::default();
    for message in messages {
        if report.matches.len() >= limit {
            break;
        }
        report.scanned += 1;
        let explanation = rule.explain(message, env, None);
        if !explanation.matched {
            continue;
        }
        let mut state = MessageState::of(message);
        let (planned, _) = rule.plan(message, &mut state, env);
        report.matches.push(TestMatch {
            message_id: message.id().clone(),
            actions: planned.iter().map(|p| p.action.clone()).collect(),
            planned,
            hits: explanation.hits,
        });
    }
    report
}
