//! Filter rules (design.md, "Filters & rules"; ADR 0022).
//!
//! A [`Rule`] is trigger(s) → scope → [`Condition`] → [`RuleAction`]s. The
//! rules live in `rules.kdl` in the config directory (the source of truth,
//! like `accounts.kdl`); this module is the typed model the UI edits and
//! the core evaluates. Folders are referenced by path or role
//! ([`FolderRef`]), as written in the file, never by opaque folder id.
//!
//! Every action of a run goes through the operation queue; a run
//! ([`RuleRun`]) can be undone as one unit
//! ([`crate::Request::UndoRuleRun`]).
//!
//! ```kdl
//! rule "Newsletters" trigger="arrival" {
//!     when any {
//!         header "List-Id" exists=#true
//!         query "from:*@substack.com"
//!     }
//!     then {
//!         tag "newsletter"
//!         move "Reading"
//!         notify #false
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::SourceLocation;
use crate::domain::{FolderRole, MessageSummary};
use crate::ids::{AccountId, IdentityId, OpId, RuleId, RuleRunId};

/// A filter rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Rule {
    /// Stable id; in [`crate::Request::SaveRule`] an empty id asks the core
    /// to derive one from the name.
    pub id: RuleId,
    /// Display name (`rule "<name>"`).
    pub name: String,
    /// Disabled rules are kept but never run automatically.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// When the rule runs. A rule can always be run by hand
    /// ([`crate::Request::RunRule`]), whatever its triggers.
    #[serde(default)]
    pub triggers: Vec<RuleTrigger>,
    /// Where it applies; empty = every account and folder.
    #[serde(default)]
    pub scope: Vec<RuleScope>,
    /// What a message must match (`when`). `all` with no children matches
    /// every message.
    #[serde(default = "Condition::always")]
    pub conditions: Condition,
    /// What happens to a matching message (`then`), in order.
    #[serde(default)]
    pub actions: Vec<RuleAction>,
    /// Where the rule is written in `rules.kdl`; `null` for a rule that is
    /// not saved yet.
    #[serde(default)]
    pub source: Option<SourceLocation>,
}

fn default_true() -> bool {
    true
}

/// When a rule runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RuleTrigger {
    /// When a new message arrives.
    Arrival,
    /// When a message is sent (applies to the sent copy).
    OnSend,
    /// Periodically over the scope.
    Schedule {
        /// Minutes between runs.
        interval_minutes: u32,
    },
    /// Only by hand, on a selection or a folder.
    Manual,
    /// Once a message is older than a number of days (checked periodically).
    OlderThan {
        /// Age in days.
        days: u32,
    },
}

/// Part of a rule's scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RuleScope {
    /// An account, real or virtual.
    Account {
        /// The account.
        account_id: AccountId,
    },
    /// A folder (and not its subfolders).
    Folder {
        /// The folder.
        folder: FolderRef,
    },
}

/// A folder as written in `rules.kdl`: by role or by path, in a given
/// account or (`account_id: null`) in the account of the message the rule
/// acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FolderRef {
    /// The folder with a special-use role (Inbox, Archive, …).
    Role {
        /// The role.
        role: FolderRole,
        /// The account; `null` = the message's account.
        #[serde(default)]
        account_id: Option<AccountId>,
    },
    /// A folder by its full path, e.g. `"Lists/Rust"`.
    Path {
        /// The path, `/`-separated.
        path: String,
        /// The account; `null` = the message's account.
        #[serde(default)]
        account_id: Option<AccountId>,
    },
}

/// A condition on a message; conditions nest with `all` / `any` / `not`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Condition {
    /// Every child matches (true when empty).
    All {
        /// The children.
        conditions: Vec<Condition>,
    },
    /// At least one child matches (false when empty).
    Any {
        /// The children.
        conditions: Vec<Condition>,
    },
    /// The child does not match.
    Not {
        /// The negated condition.
        condition: Box<Condition>,
    },
    /// A header: present (`matches: null`) or with a matching value (any of
    /// its occurrences, decoded).
    Header {
        /// Header name, case-insensitive.
        name: String,
        /// Value test; `null` = the header exists.
        #[serde(default)]
        matches: Option<TextMatch>,
    },
    /// An address field (any address in it; name or address).
    Address {
        /// Which field.
        field: AddressField,
        /// Value test.
        matches: TextMatch,
    },
    /// The subject.
    Subject {
        /// Value test.
        matches: TextMatch,
    },
    /// The decoded body text.
    Body {
        /// Value test.
        matches: TextMatch,
    },
    /// A query in the search language (design.md, "Threading and search").
    Query {
        /// The query text.
        query: String,
    },
    /// Attachments: any attachment when both tests are `null`, else one
    /// attachment passing every given test.
    Attachment {
        /// File name test.
        #[serde(default)]
        name: Option<TextMatch>,
        /// MIME type test.
        #[serde(default)]
        mime: Option<TextMatch>,
    },
    /// Message size.
    Size {
        /// Larger or smaller than.
        cmp: Comparison,
        /// Size in bytes.
        bytes: u64,
    },
    /// The message date against a calendar date.
    Date {
        /// Before or after.
        cmp: DateComparison,
        /// `YYYY-MM-DD`, local time.
        date: String,
    },
    /// The message's age.
    Age {
        /// Older (`greater`) or newer (`less`) than.
        cmp: Comparison,
        /// Age in days.
        days: u32,
    },
    /// A system flag is set (`set: true`) or not.
    Flag {
        /// The flag.
        flag: MessageFlag,
        /// Wanted state.
        set: bool,
    },
    /// The message has a tag (IMAP keyword), case-insensitive.
    Tag {
        /// The tag.
        tag: String,
    },
    /// The message is in an account (real, or a member of a virtual one).
    Account {
        /// The account.
        account_id: AccountId,
    },
    /// The message is in a folder.
    Folder {
        /// The folder.
        folder: FolderRef,
    },
    /// An `Authentication-Results` verdict.
    Auth {
        /// DKIM, SPF or DMARC.
        method: AuthMethod,
        /// Wanted result.
        result: AuthResult,
    },
    /// The `List-Id` header: present (`matches: null`) or matching.
    ListId {
        /// Value test; `null` = the message comes from a list.
        #[serde(default)]
        matches: Option<TextMatch>,
    },
}

impl Condition {
    /// `all` without children: matches every message.
    pub fn always() -> Self {
        Self::All {
            conditions: Vec::new(),
        }
    }
}

/// A text test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TextMatch {
    /// How to compare.
    pub op: TextOp,
    /// The value (a regular expression for `regex`).
    pub value: String,
    /// Compare case-sensitively (default: no).
    #[serde(default)]
    pub case_sensitive: bool,
}

/// Text comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum TextOp {
    /// The whole value is equal.
    Equals,
    /// The value contains the text.
    Contains,
    /// The value starts with the text.
    StartsWith,
    /// The value ends with the text.
    EndsWith,
    /// The value matches the regular expression (Rust `regex` syntax,
    /// unanchored).
    Regex,
}

/// Numeric comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Comparison {
    /// Strictly greater than.
    Greater,
    /// Strictly less than.
    Less,
}

/// Date comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DateComparison {
    /// Before the start of the date.
    Before,
    /// After the end of the date.
    After,
}

/// Address fields a condition can test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AddressField {
    /// `From`.
    From,
    /// `To`.
    To,
    /// `Cc`.
    Cc,
    /// `To` or `Cc`.
    Recipients,
    /// `Reply-To`.
    ReplyTo,
    /// `Sender`.
    Sender,
}

/// System flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum MessageFlag {
    /// `\Seen`.
    Seen,
    /// `\Flagged`.
    Flagged,
    /// `\Answered`.
    Answered,
    /// `\Draft`.
    Draft,
}

/// Sender authentication methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMethod {
    /// DKIM signature.
    Dkim,
    /// SPF.
    Spf,
    /// DMARC.
    Dmarc,
}

/// Authentication results (RFC 8601).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AuthResult {
    /// Passed.
    Pass,
    /// Failed.
    Fail,
    /// SPF soft fail.
    Softfail,
    /// No policy statement.
    Neutral,
    /// Not checked / no result.
    None,
    /// Temporary error.
    Temperror,
    /// Permanent error.
    Permerror,
}

/// What a rule does with a matching message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RuleAction {
    /// Move to a folder.
    Move {
        /// Target.
        folder: FolderRef,
    },
    /// Copy to a folder.
    Copy {
        /// Target.
        folder: FolderRef,
    },
    /// Delete: to Trash, or for good.
    Delete {
        /// Delete for good instead of moving to Trash.
        #[serde(default)]
        permanent: bool,
    },
    /// Move to the account's Archive.
    Archive,
    /// Add a tag.
    Tag {
        /// The tag.
        tag: String,
    },
    /// Remove a tag.
    Untag {
        /// The tag.
        tag: String,
    },
    /// Set or clear `\Flagged`.
    Flag {
        /// New state.
        flagged: bool,
    },
    /// Set or clear `\Seen`.
    MarkRead {
        /// New state.
        read: bool,
    },
    /// Forward as a new message.
    Forward {
        /// Recipients.
        to: Vec<String>,
        /// Sending identity; `null` = the account's default.
        #[serde(default)]
        identity_id: Option<IdentityId>,
    },
    /// Redirect (resend unchanged, `Resent-*` headers).
    Redirect {
        /// Recipients.
        to: Vec<String>,
    },
    /// Reply with a template.
    Reply {
        /// Template text (Markdown; variables as in composer templates).
        template: String,
        /// Sending identity; `null` = the one the message was addressed to.
        #[serde(default)]
        identity_id: Option<IdentityId>,
    },
    /// Show (or suppress) the new-mail notification.
    Notify {
        /// `false` suppresses it.
        notify: bool,
    },
    /// Stop processing further rules for this message.
    Stop,
}

/// A message a rule matched in a dry run ([`crate::Request::TestRule`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct RuleMatch {
    /// The message.
    pub message: MessageSummary,
    /// The actions that would run, in order (up to a `stop`).
    pub actions: Vec<RuleAction>,
}

/// State of a rule run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RuleRunState {
    /// Evaluating and queueing actions.
    Running,
    /// Finished; its operations are queued (see [`RuleRun::op_ids`]).
    Done,
    /// Stopped by an error.
    Failed,
    /// Undone ([`crate::Request::UndoRuleRun`]).
    Undone,
}

/// What started a rule run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RuleRunCause {
    /// New mail arrived.
    Arrival,
    /// A message was sent.
    OnSend,
    /// The schedule or the age check.
    Schedule,
    /// The user ([`crate::Request::RunRule`]).
    Manual,
}

/// One run of a rule: the unit shown in the history and undone together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RuleRun {
    /// The run.
    pub id: RuleRunId,
    /// The rule.
    pub rule_id: RuleId,
    /// The rule's name at the time of the run.
    pub rule_name: String,
    /// What started it.
    pub cause: RuleRunCause,
    /// Current state.
    pub state: RuleRunState,
    /// Start (RFC 3339).
    pub started_at: String,
    /// End (RFC 3339), `null` while running.
    pub finished_at: Option<String>,
    /// Messages matched.
    pub matched: u32,
    /// Operations the run queued.
    pub op_ids: Vec<OpId>,
    /// Whether [`crate::Request::UndoRuleRun`] can still undo it (not for
    /// sent forwards / replies, permanent deletes, or runs already undone).
    pub undoable: bool,
    /// Diagnostic detail when `failed` (English), else `null`.
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn rules_use_tagged_json() {
        let rule = Rule {
            id: "newsletters".into(),
            name: "Newsletters".into(),
            enabled: true,
            triggers: vec![RuleTrigger::Arrival, RuleTrigger::OlderThan { days: 30 }],
            scope: vec![RuleScope::Account {
                account_id: "work".into(),
            }],
            conditions: Condition::Any {
                conditions: vec![
                    Condition::ListId { matches: None },
                    Condition::Not {
                        condition: Box::new(Condition::Query {
                            query: "from:*@substack.com".into(),
                        }),
                    },
                ],
            },
            actions: vec![
                RuleAction::Tag {
                    tag: "newsletter".into(),
                },
                RuleAction::Move {
                    folder: FolderRef::Path {
                        path: "Reading".into(),
                        account_id: None,
                    },
                },
                RuleAction::Notify { notify: false },
                RuleAction::Stop,
            ],
            source: None,
        };
        let value = serde_json::to_value(&rule).unwrap();
        assert_eq!(
            value["triggers"],
            json!([{"type": "arrival"}, {"type": "older-than", "days": 30}])
        );
        assert_eq!(value["conditions"]["type"], json!("any"));
        assert_eq!(
            value["conditions"]["conditions"][1]["condition"],
            json!({"type": "query", "query": "from:*@substack.com"})
        );
        assert_eq!(
            value["actions"][1],
            json!({"type": "move", "folder": {"type": "path", "path": "Reading", "account_id": null}})
        );
        assert_eq!(value["actions"][3], json!({"type": "stop"}));
        assert_eq!(serde_json::from_value::<Rule>(value).unwrap(), rule);
    }

    #[test]
    fn rule_defaults() {
        let rule: Rule = serde_json::from_value(json!({"id": "", "name": "x"})).unwrap();
        assert!(rule.enabled);
        assert_eq!(rule.conditions, Condition::always());
        assert!(rule.triggers.is_empty() && rule.actions.is_empty());
        let m: TextMatch =
            serde_json::from_value(json!({"op": "starts-with", "value": "a"})).unwrap();
        assert!(!m.case_sensitive);
        assert_eq!(
            serde_json::to_value(RuleRunState::Undone).unwrap(),
            json!("undone")
        );
    }
}
