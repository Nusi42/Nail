//! Filter rules (M1b WP22; design.md "Filters & rules"; ADR 0022).
//!
//! The engine evaluates [`nail_api::rules::Rule`]s against messages and
//! **plans** what they do; the core executes the plan through the
//! operation queue and records it as a rule run, so a run can be undone as
//! one unit.
//!
//! * [`MessageView`] / [`Mailbox`] / [`Env`] — what rules see: a message
//!   (headers, addresses, subject, body, attachments, size, date, flags,
//!   tags, account, folder), the accounts and folders around it (virtual
//!   accounts, folder roles and paths) and the clock. [`MessageData`] and
//!   [`StaticMailbox`] are in-memory implementations.
//! * [`CompiledRule`] — a validated rule ([`validate`] lists every
//!   problem: invalid regular expressions, search queries, dates, …) with
//!   triggers ([`CompiledRule::runs_on`]: arrival, on send, schedule /
//!   older-than, manual), scope, nested all / any / not conditions
//!   ([`CompiledRule::matches`], [`CompiledRule::explain`]) and schedules
//!   ([`CompiledRule::next_run_at`]).
//! * [`RuleSet::process`] — every triggered rule in file order on one
//!   message; later rules see what earlier ones did; `stop` and deletes end
//!   processing. [`run_rule`] runs one rule on a set of messages.
//! * [`plan_actions`] — actions become [`PlannedAction`]s: queue operations
//!   ([`nail_store::Operation`]) with the operations that undo them,
//!   copies, forwards / redirects / template replies (no reply to automatic
//!   or list mail, RFC 3834; nothing sent for mail a rule sent, bounces or
//!   automatic responses, so rules cannot loop), and the notify choice.
//! * [`test_rule`] — the dry run behind "Test rule": matches, what would
//!   happen and why each matched ([`ConditionHit`]s).
//! * [`rule_from_message`] — "create rule from message" from the full
//!   header list: the mailing list (`List-Id`, `List-Post`,
//!   `X-Mailing-List`), else the sender, else the subject (M2c WP53).
//! * Problems ([`RuleIssue`], [`ConditionError`]) carry an English
//!   diagnostic and a [`nail_api::LocalizedMessage`] (`rule-issue-*` in
//!   `locales/<lang>/core.ftl`; ADR 0043).
//!
//! The `query` condition evaluates the search language (parsed by
//! `nail-search`) against the message itself, without the index.

#![warn(missing_docs)]

mod condition;
mod engine;
mod message;
mod plan;
mod query;
mod suggest;
mod text;
mod time;

pub use condition::{CompiledCondition, ConditionError, ConditionHit, ConditionPath};
pub use engine::{
    AppliedRule, CompiledRule, Explanation, IssueLocation, MessageOutcome, RuleIssue, RuleSet,
    TestMatch, TestReport, run_rule, test_rule, validate,
};
pub use suggest::rule_from_message;

pub use message::{
    AccountInfo, Attachment, Env, FolderInfo, Mailbox, MessageData, MessageView, StaticMailbox,
};
pub use plan::{
    Effect, Flow, MessageState, PlannedAction, SkipReason, is_rule_mail, plan_actions,
    refuses_automatic_forward, refuses_automatic_reply,
};
pub use text::{Matcher, REGEX_SIZE_LIMIT, glob_match};
pub use time::{days_from_civil, parse_day};

#[cfg(test)]
mod catalog_tests {
    /// Every `rule-issue-*` id built here has an English text in core.ftl.
    #[test]
    fn every_message_id_is_in_the_catalog() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let missing = nail_config::l10n::missing_message_ids(&src, &["rule-issue-"]);
        assert!(missing.is_empty(), "{missing:#?}");
    }
}
