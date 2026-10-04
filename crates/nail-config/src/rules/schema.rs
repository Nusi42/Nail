//! JSON Schema of `rules.kdl`.
//!
//! Like the other config schemas, it describes the file as a JSON object:
//! `rule` maps each rule name to its settings; `when` holds the condition
//! tree (each condition an object with one key, its node name) and `then`
//! the list of actions. The types below exist only to generate the schema;
//! the reader produces `nail_api::rules` types directly.
//! `tests/rules.rs` checks that the node and property names the reader
//! accepts are in the schema.

#![expect(dead_code, reason = "schema-only mirror types are never constructed")]

use indexmap::IndexMap;
use schemars::{JsonSchema, Schema, generate::SchemaSettings};
use serde_json::Value;

/// JSON Schema of `rules.kdl`.
pub fn rules_schema() -> Schema {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<RulesFileSchema>();
    schema.insert(
        "title".to_owned(),
        Value::String("Nail rules.kdl".to_owned()),
    );
    schema
}

/// A `rules.kdl` file: filter rules, run in file order.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct RulesFileSchema {
    /// `rule "<name>" … { trigger … scope … when … then … }`.
    #[serde(default)]
    rule: IndexMap<String, RuleSchema>,
}

/// A rule id: lowercase letters, digits, `-`, `_`, `.` (default: derived
/// from the name).
#[derive(JsonSchema)]
struct RuleIdSchema(#[schemars(pattern(r"^[a-z0-9][a-z0-9._-]{0,63}$"))] String);

/// `rule "<name>" id=… enabled=… trigger=… scope=… { … }`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct RuleSchema {
    /// Stable id (default: derived from the name).
    id: Option<RuleIdSchema>,
    /// `#false` keeps the rule without running it automatically.
    enabled: Option<bool>,
    /// `trigger="arrival"` on the rule, or `trigger "<kind>" …` children:
    /// when the rule runs. Every rule can also be run by hand.
    #[serde(default)]
    trigger: Vec<TriggerSchema>,
    /// `scope="account:<id>"` on the rule, or `scope "<spec>" account=…`
    /// children: where the rule applies (default: everywhere).
    #[serde(default)]
    scope: Vec<ScopeSchema>,
    /// `when [all|any] { … }`: what a message must match (default: every
    /// message).
    when: Option<WhenSchema>,
    /// `then { … }`: what happens to a matching message, in order.
    #[serde(default)]
    then: Vec<ActionSchema>,
}

/// A trigger.
#[derive(JsonSchema)]
#[serde(untagged)]
enum TriggerSchema {
    /// `arrival`, `on-send` or `manual`.
    Simple(SimpleTriggerSchema),
    /// `trigger "schedule" every=<minutes | "30m" | "2h" | "1d">`.
    Schedule {
        /// Always `schedule`.
        kind: ScheduleKind,
        /// Minutes between runs, or a duration string.
        every: MinutesOrDuration,
    },
    /// `trigger "older-than" days=<n>`.
    OlderThan {
        /// Always `older-than`.
        kind: OlderThanKind,
        /// Age in days.
        #[schemars(range(min = 1))]
        days: u32,
    },
}

#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum SimpleTriggerSchema {
    /// When a new message arrives.
    Arrival,
    /// When a message is sent (applies to the sent copy).
    OnSend,
    /// Only by hand.
    Manual,
}

#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum ScheduleKind {
    Schedule,
}

#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum OlderThanKind {
    OlderThan,
}

#[derive(JsonSchema)]
#[serde(untagged)]
enum MinutesOrDuration {
    Minutes(#[schemars(range(min = 1))] u32),
    Duration(#[schemars(pattern(r"^\s*[0-9]+\s*[mhd]\s*$"))] String),
}

/// `"account:<id>"`, `"virtual:<id>"`, `"folder:<path>"` or
/// `"role:<inbox|sent|drafts|archive|junk|trash>"`; folder and role scopes
/// take an optional `account=`.
#[derive(JsonSchema)]
struct ScopeSchema {
    /// The scope.
    #[schemars(pattern(r"^(account|virtual|folder|role):.+$"))]
    spec: String,
    /// The account of a folder or role scope (default: every account).
    account: Option<String>,
}

/// `when [all|any] { … }`.
#[derive(JsonSchema)]
struct WhenSchema {
    /// `all` (default: every condition must match) or `any` (one is
    /// enough).
    mode: Option<GroupMode>,
    /// The conditions.
    #[serde(default)]
    conditions: Vec<ConditionSchema>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum GroupMode {
    All,
    Any,
}

/// A text comparison: exactly one of `equals`, `contains`, `starts-with`,
/// `ends-with`, `regex` (Rust regex syntax), plus `case-sensitive`
/// (default `#false`).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct TextMatchSchema {
    equals: Option<String>,
    contains: Option<String>,
    starts_with: Option<String>,
    ends_with: Option<String>,
    regex: Option<String>,
    case_sensitive: Option<bool>,
}

/// `header "<Name>" exists=#true` or `header "<Name>" <comparison>`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct HeaderSchema {
    /// The header name.
    name: String,
    /// `#true`: present; `#false`: absent.
    exists: Option<bool>,
    #[serde(flatten)]
    matches: TextMatchSchema,
}

/// `attachment { name …; type … }`: any attachment, or one whose name and
/// MIME type match.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct AttachmentSchema {
    /// File name comparison.
    name: Option<TextMatchSchema>,
    /// MIME type comparison (`type starts-with="image/"`).
    #[serde(rename = "type")]
    mime: Option<TextMatchSchema>,
}

/// Bytes, or a string with a 1024-based unit (`"500k"`, `"5MB"`).
#[derive(JsonSchema)]
#[serde(untagged)]
enum SizeValue {
    Bytes(u64),
    WithUnit(#[schemars(pattern(r"^\s*[0-9]+\s*[kKmMgG]?[bB]?\s*$"))] String),
}

/// `size larger=… ` or `size smaller=…`.
#[derive(JsonSchema)]
struct SizeSchema {
    larger: Option<SizeValue>,
    smaller: Option<SizeValue>,
}

/// `date before="YYYY-MM-DD"` or `date after="YYYY-MM-DD"`.
#[derive(JsonSchema)]
struct DateSchema {
    #[schemars(pattern(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$"))]
    before: Option<String>,
    #[schemars(pattern(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$"))]
    after: Option<String>,
}

/// `age older-than=<days>` or `age newer-than=<days>`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct AgeSchema {
    older_than: Option<u32>,
    newer_than: Option<u32>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum FlagName {
    Seen,
    Flagged,
    Answered,
    Draft,
}

/// `flag "<flag>" set=#false`.
#[derive(JsonSchema)]
struct FlagSchema {
    flag: FlagName,
    /// Wanted state (default `#true`).
    set: Option<bool>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum RoleName {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Junk,
    Trash,
}

/// A folder: `"<path>"` or `role="<role>"`, with an optional `account=`
/// (default: the message's account).
#[derive(JsonSchema)]
struct FolderSchema {
    path: Option<String>,
    role: Option<RoleName>,
    account: Option<String>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum AuthResultName {
    Pass,
    Fail,
    Softfail,
    Neutral,
    None,
    Temperror,
    Permerror,
}

/// A condition node.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum ConditionSchema {
    /// Every child matches.
    All(Vec<ConditionSchema>),
    /// At least one child matches.
    Any(Vec<ConditionSchema>),
    /// The child (or all children together) does not match.
    Not(Vec<ConditionSchema>),
    /// A header.
    Header(HeaderSchema),
    /// Any `From` address or name.
    From(TextMatchSchema),
    /// Any `To` address or name.
    To(TextMatchSchema),
    /// Any `Cc` address or name.
    Cc(TextMatchSchema),
    /// Any `To` or `Cc` address or name.
    Recipients(TextMatchSchema),
    /// Any `Reply-To` address or name.
    ReplyTo(TextMatchSchema),
    /// The `Sender` address or name.
    Sender(TextMatchSchema),
    /// The subject.
    Subject(TextMatchSchema),
    /// The decoded body text.
    Body(TextMatchSchema),
    /// A query in the search language.
    Query(String),
    /// Attachments.
    Attachment(AttachmentSchema),
    /// Message size.
    Size(SizeSchema),
    /// Message date.
    Date(DateSchema),
    /// Message age in days.
    Age(AgeSchema),
    /// A system flag.
    Flag(FlagSchema),
    /// A tag.
    Tag(String),
    /// An account (real or virtual).
    Account(String),
    /// A folder.
    Folder(FolderSchema),
    /// DKIM result from `Authentication-Results`.
    Dkim(AuthResultName),
    /// SPF result.
    Spf(AuthResultName),
    /// DMARC result.
    Dmarc(AuthResultName),
    /// The `List-Id` header: present, or matching.
    ListId(Option<TextMatchSchema>),
}

/// `forward "<address>"… identity=…`.
#[derive(JsonSchema)]
struct ForwardSchema {
    to: Vec<String>,
    identity: Option<String>,
}

/// `reply "<template>" identity=…`.
#[derive(JsonSchema)]
struct ReplySchema {
    /// Markdown template text.
    template: String,
    identity: Option<String>,
}

/// `delete permanent=#true`.
#[derive(JsonSchema)]
struct DeleteSchema {
    /// Delete for good instead of moving to Trash.
    permanent: Option<bool>,
}

/// An action node.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum ActionSchema {
    /// Move to a folder.
    Move(FolderSchema),
    /// Copy to a folder.
    Copy(FolderSchema),
    /// Delete (to Trash, or for good).
    Delete(DeleteSchema),
    /// Move to the account's Archive.
    Archive,
    /// Add a tag.
    Tag(String),
    /// Remove a tag.
    Untag(String),
    /// `flag` / `flag #false`.
    Flag(Option<bool>),
    /// `mark-read` / `mark-read #false`.
    MarkRead(Option<bool>),
    /// Forward as a new message.
    Forward(ForwardSchema),
    /// Redirect unchanged.
    Redirect(Vec<String>),
    /// Reply with a template.
    Reply(ReplySchema),
    /// `notify` / `notify #false` (suppress the new-mail notification).
    Notify(Option<bool>),
    /// Stop processing further rules for this message.
    Stop,
}
