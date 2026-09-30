//! Rules and vacation settings → one Sieve script per account (RFC 5228 +
//! `fileinto`, `imap4flags`, `copy`, `variables`, `body`, `regex`,
//! `relational`, `date`, `index`, `vacation`, `include`), plus where each
//! rule runs ([`RuleSieveStatus`]).
//!
//! # Semantics
//!
//! A rule runs on the **server** when it is enabled, runs on arrival (and
//! otherwise only by hand), covers the account's Inbox (Sieve sees
//! deliveries only), every condition and action has a Sieve equivalent,
//! and the server has the extensions they need. It is **not expressible**
//! when a condition or action has no Sieve equivalent (search queries,
//! attachments, age, flags and tags of the stored message, folders other
//! than the Inbox, forwards, template replies, notifications, moves into
//! another account, regular expressions beyond POSIX ERE); otherwise it is
//! a **client** rule with the reasons why.
//!
//! Nail's rule engine applies actions to one message in order and the last
//! move wins. Sieve would file a copy for every `fileinto`. To keep Nail's
//! meaning, a script with server rules that move, archive or delete
//! records the target in variables (`nail_deliver`, `nail_folder`) and
//! files the message once at the end (and before any `stop`). Flags and
//! tags go to the `imap4flags` internal variable, so they apply to the
//! final delivery wherever they occur; a copy (`fileinto :copy`) carries
//! the flags set before it, as in Nail.
//!
//! See ADR 0044 (`docs/adr/0044-m2c-sieve.md`) for the approximations (address display
//! names, `Authentication-Results`, case folding).

use std::collections::{BTreeSet, HashMap};

use nail_api::domain::FolderRole;
use nail_api::ids::AccountId;
use nail_api::localized::LocalizedMessage;
use nail_api::rules::{
    AddressField, AuthMethod, AuthResult, Comparison, Condition, DateComparison, FolderRef, Rule,
    RuleAction, RuleScope, RuleTrigger, TextMatch, TextOp,
};
use nail_api::sieve::{RulePlacement, RuleSieveStatus, VacationSettings, is_iso_date};

use crate::ere::to_posix_ere;
use crate::reasons;
use crate::writer::{DOLLAR_VAR, Escaper, matches_literal, multiline, quote};

/// First line of every script Nail writes; a script with Nail's name but
/// without it belongs to the user and is never overwritten.
pub const MANAGED_MARKER: &str = "# nail-managed-script";

/// Comment line recording the script that was active before Nail's
/// (restored when Nail's script is removed).
pub const PREVIOUS_MARKER: &str = "# nail:previous ";

/// Comment line before each rule block: `# rule "<id>"`.
pub(crate) const RULE_MARKER: &str = "# rule ";

/// Variable with the pending delivery (`fileinto` / `discard`).
pub(crate) const DELIVER_VAR: &str = "nail_deliver";
/// Variable with the pending target mailbox.
pub(crate) const FOLDER_VAR: &str = "nail_folder";

/// The extensions most ManageSieve servers offer (Dovecot Pigeonhole,
/// Cyrus, Stalwart), for previews before Nail has connected.
pub const COMMON_EXTENSIONS: &[&str] = &[
    "fileinto",
    "imap4flags",
    "copy",
    "variables",
    "body",
    "regex",
    "relational",
    "date",
    "index",
    "vacation",
    "include",
    "comparator-i;unicode-casemap",
];

/// Names the account's folders on the server.
pub trait FolderResolver {
    /// The mailbox name (as `fileinto` expects it: full name, UTF-8, the
    /// server's hierarchy delimiter) of `folder` in the account the script
    /// is for. `folder.account_id` is already checked. `None` = unknown
    /// (e.g. no folder has the role yet).
    fn mailbox(&self, folder: &FolderRef) -> Option<String>;
}

/// A [`FolderResolver`] from a role table and a hierarchy delimiter.
#[derive(Debug, Clone, Default)]
pub struct FolderMap {
    /// Mailbox names of the special-use folders (`INBOX` is implied).
    pub roles: HashMap<FolderRole, String>,
    /// The server's hierarchy delimiter; `None` = `/`.
    pub delimiter: Option<char>,
}

impl FolderMap {
    /// A map with the given roles and delimiter.
    pub fn new(delimiter: Option<char>, roles: &[(FolderRole, &str)]) -> Self {
        Self {
            roles: roles.iter().map(|(r, n)| (*r, (*n).to_owned())).collect(),
            delimiter,
        }
    }
}

impl FolderResolver for FolderMap {
    fn mailbox(&self, folder: &FolderRef) -> Option<String> {
        match folder {
            FolderRef::Role {
                role: FolderRole::Inbox,
                ..
            } => Some("INBOX".to_owned()),
            FolderRef::Role { role, .. } => self.roles.get(role).cloned(),
            FolderRef::Path { path, .. } => {
                let delimiter = self.delimiter.unwrap_or('/');
                let parts: Vec<&str> = path.split('/').collect();
                if path.is_empty()
                    || parts
                        .iter()
                        .any(|p| p.is_empty() || (delimiter != '/' && p.contains(delimiter)))
                {
                    return None;
                }
                Some(parts.join(&delimiter.to_string()))
            }
        }
    }
}

/// What the compiler needs to know about the account and its server.
pub struct CompileOptions<'a> {
    /// The account the script is for.
    pub account_id: AccountId,
    /// Virtual accounts that include this account (a rule scoped to one of
    /// them applies here too).
    pub account_aliases: Vec<AccountId>,
    /// The server's `SIEVE` extensions (any case). Use
    /// [`COMMON_EXTENSIONS`] before the first connection.
    pub extensions: Vec<String>,
    /// Mailbox names of the account's folders.
    pub folders: &'a dyn FolderResolver,
    /// The account's own addresses (identities) for the vacation reply's
    /// `:addresses`.
    pub own_addresses: Vec<String>,
    /// The account's current UTC offset in minutes, for `date` tests and
    /// vacation dates (`:zone`).
    pub utc_offset_minutes: i32,
    /// Today (`YYYY-MM-DD`, the account's local time): vacation dates are
    /// applied by Nail when the server lacks the `date` extension.
    pub today: Option<String>,
    /// The server's `MAXREDIRECTS`.
    pub max_redirects: Option<u32>,
    /// The script that was active before Nail's (another script of the
    /// user). It is recorded in the script and, when the server has
    /// `include`, still run (`include :personal`) so activating Nail's
    /// script does not switch it off.
    pub previous_active: Option<String>,
}

impl std::fmt::Debug for CompileOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompileOptions")
            .field("account_id", &self.account_id)
            .field("extensions", &self.extensions)
            .field("previous_active", &self.previous_active)
            .finish_non_exhaustive()
    }
}

/// Where the vacation reply stands in a compiled script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VacationState {
    /// Switched off (or invalid settings).
    Off,
    /// In the script and answering (dates checked by the server when it
    /// has `date`).
    Active,
    /// Not in the script yet: the start date is in the future and the
    /// server lacks `date` (see [`VacationOutcome::recompile_on`]).
    Waiting,
    /// Not in the script: the end date has passed (server without `date`).
    Ended,
    /// The server has no `vacation` extension.
    Unsupported,
}

/// The vacation part of a compiled script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VacationOutcome {
    /// Where it stands.
    pub state: VacationState,
    /// The day (`YYYY-MM-DD`) on which the script must be compiled and
    /// uploaded again because Nail applies the dates itself (server
    /// without `date`); `None` when nothing is scheduled.
    pub recompile_on: Option<String>,
}

impl VacationOutcome {
    /// Whether the script contains an active vacation reply.
    pub fn in_script(&self) -> bool {
        self.state == VacationState::Active
    }
}

/// A compiled script and the placement of every rule that applies to the
/// account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledScript {
    /// The script (`\n` line ends; the client sends CRLF).
    pub script: String,
    /// Placement of each rule that applies to the account, in input order.
    pub rules: Vec<RuleSieveStatus>,
    /// The extensions the script requires (sorted).
    pub required: Vec<String>,
    /// The vacation reply.
    pub vacation: VacationOutcome,
    /// The other script run through `include`, if any.
    pub included: Option<String>,
}

impl CompiledScript {
    /// Ids of the rules compiled into the script, in order.
    pub fn server_rules(&self) -> Vec<&str> {
        self.rules
            .iter()
            .filter(|r| r.placement == RulePlacement::Server)
            .map(|r| r.rule_id.as_str())
            .collect()
    }
}

/// Compiles `rules` (all of `rules.kdl`, in order) and the vacation
/// settings of one account into its script.
pub fn compile(
    rules: &[Rule],
    vacation: &VacationSettings,
    options: &CompileOptions<'_>,
) -> CompiledScript {
    let exts: BTreeSet<String> = options
        .extensions
        .iter()
        .map(|e| e.trim().to_ascii_lowercase())
        .collect();

    // Pass 1: placement of every applicable rule.
    let mut statuses = Vec::new();
    let mut server = Vec::new();
    for rule in rules {
        let coverage = coverage(rule, options);
        if coverage == Coverage::None {
            continue;
        }
        let analysis = RuleCompiler::new(options, Escaper::default()).rule(rule);
        let status = placement(rule, coverage, &analysis, &exts, options);
        if status.placement == RulePlacement::Server {
            server.push((rule, analysis));
        }
        statuses.push(status);
    }

    let vacation_plan = plan_vacation(vacation, options, &exts);

    let mut required: BTreeSet<String> = BTreeSet::new();
    for (_, analysis) in &server {
        required.extend(analysis.needs.iter().cloned());
    }
    required.extend(vacation_plan.needs.iter().cloned());
    let included = options
        .previous_active
        .clone()
        .filter(|_| exts.contains("include"));
    if included.is_some() {
        required.insert("include".to_owned());
    }
    let delivery = server.iter().any(|(_, a)| a.delivery);
    let esc = Escaper {
        variables: required.contains("variables"),
    };

    // Pass 2: the script.
    let mut out = String::new();
    out.push_str(MANAGED_MARKER);
    out.push('\n');
    if let Some(previous) = &options.previous_active {
        out.push_str(PREVIOUS_MARKER);
        out.push_str(&quote(&single_line(previous)));
        out.push('\n');
    }
    if !required.is_empty() {
        let list: Vec<&String> = required.iter().collect();
        out.push_str(&format!("require {};\n", Escaper::default().list(&list)));
    }
    if esc.variables {
        out.push_str(&format!("set \"{DOLLAR_VAR}\" \"$\";\n"));
    }
    if let Some(block) = vacation_plan.block(esc) {
        out.push_str("# nail:vacation\n");
        out.push_str(&block);
    }
    if let Some(name) = &included {
        out.push_str(&format!("include :personal {};\n", quote(name)));
    }
    for (rule, _) in &server {
        let rendered = RuleCompiler::new(options, esc).rule(rule);
        out.push_str(RULE_MARKER);
        out.push_str(&quote(&single_line(rule.id.as_str())));
        out.push('\n');
        out.push_str(&format!("if {} {{\n", rendered.test));
        for emit in &rendered.actions {
            match emit {
                Emit::Line(line) => out.push_str(&format!("    {line}\n")),
                Emit::Stop => {
                    if delivery {
                        for line in flush_lines() {
                            out.push_str(&format!("    {line}\n"));
                        }
                    }
                    out.push_str("    stop;\n");
                }
            }
        }
        out.push_str("}\n");
    }
    if delivery {
        out.push_str("# nail:deliver\n");
        for line in flush_lines() {
            out.push_str(&line);
            out.push('\n');
        }
    }

    CompiledScript {
        script: out,
        rules: statuses,
        required: required.into_iter().collect(),
        vacation: vacation_plan.outcome,
        included,
    }
}

/// The final delivery of a script with deferred moves.
fn flush_lines() -> Vec<String> {
    vec![
        format!("if string :is \"${{{DELIVER_VAR}}}\" \"fileinto\" {{"),
        format!("    fileinto \"${{{FOLDER_VAR}}}\";"),
        format!("}} elsif string :is \"${{{DELIVER_VAR}}}\" \"discard\" {{"),
        "    discard;".to_owned(),
        "}".to_owned(),
    ]
}

fn single_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

// ---------------------------------------------------------------------------
// Scope and triggers

/// How much of the account a rule's scope covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coverage {
    /// The rule does not apply to the account.
    None,
    /// The whole account.
    Whole,
    /// The Inbox only (all Sieve sees).
    Inbox,
    /// Folders other than the Inbox.
    OtherFolders,
}

fn is_this_account(id: &AccountId, options: &CompileOptions<'_>) -> bool {
    *id == options.account_id || options.account_aliases.contains(id)
}

fn folder_account_ok(folder: &FolderRef, options: &CompileOptions<'_>) -> bool {
    let account = match folder {
        FolderRef::Role { account_id, .. } | FolderRef::Path { account_id, .. } => account_id,
    };
    account.as_ref().is_none_or(|a| is_this_account(a, options))
}

fn is_inbox(folder: &FolderRef) -> bool {
    match folder {
        FolderRef::Role { role, .. } => *role == FolderRole::Inbox,
        FolderRef::Path { path, .. } => path.eq_ignore_ascii_case("INBOX"),
    }
}

fn coverage(rule: &Rule, options: &CompileOptions<'_>) -> Coverage {
    if rule.scope.is_empty() {
        return Coverage::Whole;
    }
    let mut result = Coverage::None;
    for scope in &rule.scope {
        match scope {
            RuleScope::Account { account_id } if is_this_account(account_id, options) => {
                return Coverage::Whole;
            }
            RuleScope::Account { .. } => {}
            RuleScope::Folder { folder } if folder_account_ok(folder, options) => {
                if is_inbox(folder) {
                    if result == Coverage::None {
                        result = Coverage::Inbox;
                    }
                } else {
                    result = Coverage::OtherFolders;
                }
            }
            RuleScope::Folder { .. } => {}
        }
    }
    result
}

fn placement(
    rule: &Rule,
    coverage: Coverage,
    analysis: &RuleOutput,
    exts: &BTreeSet<String>,
    options: &CompileOptions<'_>,
) -> RuleSieveStatus {
    let status = |placement, reasons| RuleSieveStatus {
        rule_id: rule.id.clone(),
        placement,
        reasons,
    };
    if !analysis.not_expressible.is_empty() {
        return status(
            RulePlacement::NotExpressible,
            analysis.not_expressible.clone(),
        );
    }
    let mut reasons = Vec::new();
    if !rule.enabled {
        reasons.push(LocalizedMessage::new(reasons::DISABLED));
    }
    if !rule.triggers.contains(&RuleTrigger::Arrival) {
        reasons.push(LocalizedMessage::new(reasons::NO_ARRIVAL));
    }
    if rule.triggers.iter().any(|t| {
        matches!(
            t,
            RuleTrigger::OnSend | RuleTrigger::Schedule { .. } | RuleTrigger::OlderThan { .. }
        )
    }) {
        reasons.push(LocalizedMessage::new(reasons::TRIGGER));
    }
    if coverage == Coverage::OtherFolders {
        reasons.push(LocalizedMessage::new(reasons::SCOPE));
    }
    for ext in &analysis.needs {
        if !exts.contains(ext) {
            reasons.push(reasons::extension(ext));
        }
    }
    if let Some(max) = options.max_redirects
        && analysis.redirects > max as usize
    {
        reasons.push(
            LocalizedMessage::new(reasons::REDIRECTS)
                .arg("count", analysis.redirects)
                .arg("max", max),
        );
    }
    reasons.extend(analysis.client.iter().cloned());
    if reasons.is_empty() {
        status(RulePlacement::Server, Vec::new())
    } else {
        status(RulePlacement::Client, reasons)
    }
}

// ---------------------------------------------------------------------------
// Conditions and actions

/// One line of a rule block, or a `stop` (rendered with the final
/// delivery before it when moves are deferred).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Emit {
    Line(String),
    Stop,
}

/// A rule analysed and rendered.
#[derive(Debug, Default)]
struct RuleOutput {
    test: String,
    actions: Vec<Emit>,
    needs: BTreeSet<String>,
    not_expressible: Vec<LocalizedMessage>,
    client: Vec<LocalizedMessage>,
    /// Moves, archives or deletes (deferred delivery).
    delivery: bool,
    redirects: usize,
}

struct RuleCompiler<'o, 'a> {
    options: &'o CompileOptions<'a>,
    esc: Escaper,
    out: RuleOutput,
}

impl<'o, 'a> RuleCompiler<'o, 'a> {
    fn new(options: &'o CompileOptions<'a>, esc: Escaper) -> Self {
        Self {
            options,
            esc,
            out: RuleOutput::default(),
        }
    }

    fn need(&mut self, ext: &str) {
        self.out.needs.insert(ext.to_owned());
    }

    fn not_expressible(&mut self, reason: LocalizedMessage) -> String {
        self.out.not_expressible.push(reason);
        "false".to_owned()
    }

    fn rule(mut self, rule: &Rule) -> RuleOutput {
        let mut path = Vec::new();
        self.out.test = self.condition(&rule.conditions, &mut path);
        for (index, action) in rule.actions.iter().enumerate() {
            if self.action(index, action) {
                break;
            }
        }
        self.out
    }

    /// `[comparator] match-type` tags and the key for a text test.
    fn match_args(&mut self, m: &TextMatch, path: &[u32]) -> Option<(String, String)> {
        let non_ascii = !m.value.is_ascii();
        let mut tags = String::new();
        if m.case_sensitive {
            tags.push_str(":comparator \"i;octet\" ");
        } else if non_ascii {
            if m.op == TextOp::Regex {
                // Servers only apply ASCII case folding to regular
                // expressions.
                self.not_expressible(reasons::at_condition(reasons::REGEX, path));
                return None;
            }
            self.need("comparator-i;unicode-casemap");
            tags.push_str(":comparator \"i;unicode-casemap\" ");
        }
        let (match_type, key) = match m.op {
            TextOp::Equals => (":is", m.value.trim().to_owned()),
            TextOp::Contains => (":contains", m.value.clone()),
            TextOp::StartsWith | TextOp::EndsWith if m.value.is_empty() => {
                (":contains", String::new())
            }
            TextOp::StartsWith => (":matches", format!("{}*", matches_literal(&m.value))),
            TextOp::EndsWith => (":matches", format!("*{}", matches_literal(&m.value))),
            TextOp::Regex => match to_posix_ere(&m.value) {
                Some(ere) => {
                    self.need("regex");
                    (":regex", ere)
                }
                None => {
                    let id = if regex::Regex::new(&m.value).is_err() {
                        reasons::INVALID
                    } else {
                        reasons::REGEX
                    };
                    self.not_expressible(reasons::at_condition(id, path));
                    return None;
                }
            },
        };
        tags.push_str(match_type);
        Some((tags, self.esc.quoted(&key)))
    }

    fn condition(&mut self, condition: &Condition, path: &mut Vec<u32>) -> String {
        match condition {
            Condition::All { conditions } if conditions.is_empty() => "true".to_owned(),
            Condition::Any { conditions } if conditions.is_empty() => "false".to_owned(),
            Condition::All { conditions } | Condition::Any { conditions } => {
                let name = if matches!(condition, Condition::All { .. }) {
                    "allof"
                } else {
                    "anyof"
                };
                let mut parts = Vec::new();
                for (i, child) in conditions.iter().enumerate() {
                    path.push(u32::try_from(i).unwrap_or(u32::MAX));
                    parts.push(self.condition(child, path));
                    path.pop();
                }
                format!("{name}({})", parts.join(", "))
            }
            Condition::Not { condition } => {
                path.push(0);
                let inner = self.condition(condition, path);
                path.pop();
                format!("not {inner}")
            }
            Condition::Header { name, matches } => {
                let Some(name) = self.header_name(name, path) else {
                    return "false".to_owned();
                };
                match matches {
                    None => format!("exists {}", self.esc.quoted(&name)),
                    Some(m) => self.header_test(&name, m, path),
                }
            }
            Condition::Address { field, matches } => {
                let Some((tags, key)) = self.match_args(matches, path) else {
                    return "false".to_owned();
                };
                let fields = self.esc.list(address_fields(*field));
                format!("anyof(address :all {tags} {fields} {key}, header {tags} {fields} {key})")
            }
            Condition::Subject { matches } => self.header_test("subject", matches, path),
            Condition::Body { matches } => {
                let Some((tags, key)) = self.match_args(matches, path) else {
                    return "false".to_owned();
                };
                self.need("body");
                format!("body :text {tags} {key}")
            }
            Condition::Query { .. } => {
                self.not_expressible(reasons::at_condition(reasons::QUERY, path))
            }
            Condition::Attachment { .. } => {
                self.not_expressible(reasons::at_condition(reasons::ATTACHMENT, path))
            }
            Condition::Age { .. } => {
                self.not_expressible(reasons::at_condition(reasons::AGE, path))
            }
            Condition::Flag { .. } => {
                self.not_expressible(reasons::at_condition(reasons::FLAG, path))
            }
            Condition::Tag { .. } => {
                self.not_expressible(reasons::at_condition(reasons::TAG, path))
            }
            Condition::Size { cmp, bytes } => {
                let tag = match cmp {
                    Comparison::Greater => ":over",
                    Comparison::Less => ":under",
                };
                format!("size {tag} {bytes}")
            }
            Condition::Date { cmp, date } => {
                if !is_iso_date(date) {
                    return self.not_expressible(reasons::at_condition(reasons::INVALID, path));
                }
                self.need("date");
                self.need("relational");
                let rel = match cmp {
                    DateComparison::Before => "lt",
                    DateComparison::After => "gt",
                };
                format!(
                    "date :zone \"{}\" :value \"{rel}\" \"date\" \"date\" {}",
                    zone(self.options.utc_offset_minutes),
                    quote(date)
                )
            }
            Condition::Account { account_id } => {
                if is_this_account(account_id, self.options) {
                    "true".to_owned()
                } else {
                    "false".to_owned()
                }
            }
            Condition::Folder { folder } => {
                if is_inbox(folder) && folder_account_ok(folder, self.options) {
                    "true".to_owned()
                } else {
                    self.not_expressible(reasons::at_condition(reasons::FOLDER, path))
                }
            }
            Condition::Auth { method, result } => {
                self.need("index");
                format!(
                    "header :index 1 :contains \"authentication-results\" {}",
                    quote(&format!(
                        "{}={}",
                        auth_method(*method),
                        auth_result(*result)
                    ))
                )
            }
            Condition::ListId { matches: None } => "exists \"list-id\"".to_owned(),
            Condition::ListId { matches: Some(m) } => self.list_id(m, path),
        }
    }

    fn header_name(&mut self, name: &str, path: &[u32]) -> Option<String> {
        let name = name.trim();
        let valid = !name.is_empty() && name.bytes().all(|b| b.is_ascii_graphic() && b != b':');
        if valid {
            Some(name.to_ascii_lowercase())
        } else {
            self.not_expressible(reasons::at_condition(reasons::HEADER_NAME, path));
            None
        }
    }

    fn header_test(&mut self, name: &str, m: &TextMatch, path: &[u32]) -> String {
        match self.match_args(m, path) {
            Some((tags, key)) => format!("header {tags} {} {key}", self.esc.quoted(name)),
            None => "false".to_owned(),
        }
    }

    /// `List-Id`: Nail also tests the id inside the angle brackets.
    fn list_id(&mut self, m: &TextMatch, path: &[u32]) -> String {
        let plain = self.header_test("list-id", m, path);
        let inner = match m.op {
            TextOp::Equals => matches_literal(m.value.trim()),
            TextOp::StartsWith if !m.value.is_empty() => format!("{}*", matches_literal(&m.value)),
            TextOp::EndsWith if !m.value.is_empty() => format!("*{}", matches_literal(&m.value)),
            _ => return plain,
        };
        if plain == "false" {
            return plain;
        }
        let comparator = if m.case_sensitive {
            ":comparator \"i;octet\" "
        } else if !m.value.is_ascii() {
            ":comparator \"i;unicode-casemap\" "
        } else {
            ""
        };
        format!(
            "anyof({plain}, header {comparator}:matches \"list-id\" {})",
            self.esc.quoted(&format!("*<{inner}>*"))
        )
    }

    /// The mailbox of a move / copy target, or a reason.
    fn target(&mut self, index: usize, folder: &FolderRef) -> Option<String> {
        if !folder_account_ok(folder, self.options) {
            self.out
                .not_expressible
                .push(reasons::at_action(reasons::OTHER_ACCOUNT, index));
            return None;
        }
        match self.options.folders.mailbox(folder) {
            Some(mailbox) => Some(mailbox),
            None => {
                let name = match folder {
                    FolderRef::Role { role, .. } => format!("{role:?}").to_ascii_lowercase(),
                    FolderRef::Path { path, .. } => path.clone(),
                };
                self.out.client.push(
                    LocalizedMessage::new(reasons::FOLDER_UNKNOWN)
                        .arg("folder", name)
                        .arg("action", index + 1),
                );
                None
            }
        }
    }

    fn deliver_to(&mut self, index: usize, folder: &FolderRef) {
        if let Some(mailbox) = self.target(index, folder) {
            self.need("fileinto");
            self.need("variables");
            self.out.delivery = true;
            self.out
                .actions
                .push(Emit::Line(format!("set \"{DELIVER_VAR}\" \"fileinto\";")));
            self.out.actions.push(Emit::Line(format!(
                "set \"{FOLDER_VAR}\" {};",
                self.esc.quoted(&mailbox)
            )));
        }
    }

    fn flag(&mut self, add: bool, flag: &str) {
        self.need("imap4flags");
        let command = if add { "addflag" } else { "removeflag" };
        self.out
            .actions
            .push(Emit::Line(format!("{command} {};", self.esc.quoted(flag))));
    }

    /// Compiles one action; `true` after `stop`.
    fn action(&mut self, index: usize, action: &RuleAction) -> bool {
        match action {
            RuleAction::Move { folder } => self.deliver_to(index, folder),
            RuleAction::Archive => self.deliver_to(
                index,
                &FolderRef::Role {
                    role: FolderRole::Archive,
                    account_id: None,
                },
            ),
            RuleAction::Delete { permanent: false } => self.deliver_to(
                index,
                &FolderRef::Role {
                    role: FolderRole::Trash,
                    account_id: None,
                },
            ),
            RuleAction::Delete { permanent: true } => {
                self.need("variables");
                self.out.delivery = true;
                self.out
                    .actions
                    .push(Emit::Line(format!("set \"{DELIVER_VAR}\" \"discard\";")));
            }
            RuleAction::Copy { folder } => {
                if let Some(mailbox) = self.target(index, folder) {
                    self.need("fileinto");
                    self.need("copy");
                    self.out.actions.push(Emit::Line(format!(
                        "fileinto :copy {};",
                        self.esc.quoted(&mailbox)
                    )));
                }
            }
            RuleAction::Tag { tag } | RuleAction::Untag { tag } => {
                if is_keyword(tag) {
                    self.flag(matches!(action, RuleAction::Tag { .. }), tag);
                } else {
                    self.out
                        .not_expressible
                        .push(reasons::at_action(reasons::KEYWORD, index));
                }
            }
            RuleAction::Flag { flagged } => self.flag(*flagged, "\\Flagged"),
            RuleAction::MarkRead { read } => self.flag(*read, "\\Seen"),
            RuleAction::Redirect { to } => {
                if to.iter().any(|a| !is_address(a)) {
                    self.out
                        .not_expressible
                        .push(reasons::at_action(reasons::ADDRESS, index));
                } else {
                    for address in to {
                        self.need("copy");
                        self.out.redirects += 1;
                        self.out.actions.push(Emit::Line(format!(
                            "redirect :copy {};",
                            self.esc.quoted(address.trim())
                        )));
                    }
                }
            }
            RuleAction::Forward { .. } => self
                .out
                .not_expressible
                .push(reasons::at_action(reasons::FORWARD, index)),
            RuleAction::Reply { .. } => self
                .out
                .not_expressible
                .push(reasons::at_action(reasons::REPLY, index)),
            RuleAction::Notify { .. } => self
                .out
                .not_expressible
                .push(reasons::at_action(reasons::NOTIFY, index)),
            RuleAction::Stop => {
                self.out.actions.push(Emit::Stop);
                return true;
            }
        }
        false
    }
}

/// The header names of an address field.
pub(crate) fn address_fields(field: AddressField) -> &'static [&'static str] {
    match field {
        AddressField::From => &["from"],
        AddressField::To => &["to"],
        AddressField::Cc => &["cc"],
        AddressField::Recipients => &["to", "cc"],
        AddressField::ReplyTo => &["reply-to"],
        AddressField::Sender => &["sender"],
    }
}

pub(crate) fn auth_method(method: AuthMethod) -> &'static str {
    match method {
        AuthMethod::Dkim => "dkim",
        AuthMethod::Spf => "spf",
        AuthMethod::Dmarc => "dmarc",
    }
}

pub(crate) fn auth_result(result: AuthResult) -> &'static str {
    match result {
        AuthResult::Pass => "pass",
        AuthResult::Fail => "fail",
        AuthResult::Softfail => "softfail",
        AuthResult::Neutral => "neutral",
        AuthResult::None => "none",
        AuthResult::Temperror => "temperror",
        AuthResult::Permerror => "permerror",
    }
}

/// An IMAP keyword (RFC 3501 atom, not a system flag), as `imap4flags`
/// needs it.
pub(crate) fn is_keyword(tag: &str) -> bool {
    !tag.is_empty()
        && !tag.starts_with('\\')
        && tag.bytes().all(|b| {
            b.is_ascii_graphic()
                && !matches!(b, b'(' | b')' | b'{' | b'%' | b'*' | b'"' | b'\\' | b']')
        })
}

fn is_address(address: &str) -> bool {
    let a = address.trim();
    !a.is_empty()
        && a.contains('@')
        && !a.contains(char::is_whitespace)
        && !a.contains(['"', '<', '>'])
}

/// `+hhmm` / `-hhmm` for a UTC offset in minutes.
pub(crate) fn zone(offset_minutes: i32) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let m = offset_minutes.unsigned_abs();
    format!("{sign}{:02}{:02}", m / 60, m % 60)
}

// ---------------------------------------------------------------------------
// Vacation

struct VacationPlan {
    outcome: VacationOutcome,
    needs: BTreeSet<String>,
    settings: Option<VacationSettings>,
    dated: bool,
    addresses: Vec<String>,
    zone: String,
}

impl VacationPlan {
    fn block(&self, esc: Escaper) -> Option<String> {
        let settings = self.settings.as_ref()?;
        let mut tests = Vec::new();
        if self.dated {
            if let Some(start) = &settings.start_date {
                tests.push(format!(
                    "currentdate :zone \"{}\" :value \"ge\" \"date\" {}",
                    self.zone,
                    quote(start)
                ));
            }
            if let Some(end) = &settings.end_date {
                tests.push(format!(
                    "currentdate :zone \"{}\" :value \"le\" \"date\" {}",
                    self.zone,
                    quote(end)
                ));
            }
        }
        if settings.skip_lists {
            tests.push("not exists \"list-id\"".to_owned());
            tests.push("not header :is \"precedence\" [\"bulk\", \"list\", \"junk\"]".to_owned());
            tests.push("not header :matches \"auto-submitted\" \"auto-*\"".to_owned());
        }
        let mut command = format!("vacation :days {}", settings.days);
        if !settings.subject.trim().is_empty() {
            command.push_str(&format!(
                " :subject {}",
                esc.quoted(settings.subject.trim())
            ));
        }
        if let Some(from) = &settings.from {
            command.push_str(&format!(" :from {}", esc.quoted(from.trim())));
        }
        if !self.addresses.is_empty() {
            command.push_str(&format!(" :addresses {}", esc.list(&self.addresses)));
        }
        command.push(' ');
        command.push_str(&multiline(&esc.text(&settings.body)));
        command.push(';');
        Some(if tests.is_empty() {
            format!("{command}\n")
        } else {
            // The `text:` lines are content: no indentation inside.
            format!("if allof({}) {{\n    {command}\n}}\n", tests.join(", "))
        })
    }
}

fn plan_vacation(
    settings: &VacationSettings,
    options: &CompileOptions<'_>,
    exts: &BTreeSet<String>,
) -> VacationPlan {
    let mut plan = VacationPlan {
        outcome: VacationOutcome {
            state: VacationState::Off,
            recompile_on: None,
        },
        needs: BTreeSet::new(),
        settings: None,
        dated: false,
        addresses: Vec::new(),
        zone: zone(options.utc_offset_minutes),
    };
    if !settings.enabled || !settings.issues().is_empty() {
        return plan;
    }
    if !exts.contains("vacation") {
        plan.outcome.state = VacationState::Unsupported;
        return plan;
    }
    let has_dates = settings.start_date.is_some() || settings.end_date.is_some();
    if has_dates && exts.contains("date") && exts.contains("relational") {
        plan.dated = true;
        plan.needs.insert("date".to_owned());
        plan.needs.insert("relational".to_owned());
    } else if has_dates && let Some(today) = options.today.as_deref() {
        // Nail applies the dates: the script holds the reply only while
        // today is inside the range.
        if let Some(start) = settings.start_date.as_deref()
            && today < start
        {
            plan.outcome = VacationOutcome {
                state: VacationState::Waiting,
                recompile_on: Some(start.to_owned()),
            };
            return plan;
        }
        if let Some(end) = settings.end_date.as_deref() {
            if today > end {
                plan.outcome.state = VacationState::Ended;
                return plan;
            }
            plan.outcome.recompile_on = next_day(end);
        }
    }
    plan.needs.insert("vacation".to_owned());
    let mut seen = BTreeSet::new();
    for address in settings.addresses.iter().chain(&options.own_addresses) {
        let address = address.trim();
        if !address.is_empty() && seen.insert(address.to_ascii_lowercase()) {
            plan.addresses.push(address.to_owned());
        }
    }
    plan.settings = Some(settings.clone());
    plan.outcome.state = VacationState::Active;
    plan
}

/// The day after `date` (`YYYY-MM-DD`, proleptic Gregorian).
pub(crate) fn next_day(date: &str) -> Option<String> {
    if !is_iso_date(date) {
        return None;
    }
    let y: i64 = date[0..4].parse().ok()?;
    let m: i64 = date[5..7].parse().ok()?;
    let d: i64 = date[8..10].parse().ok()?;
    let (y, m, d) = civil_from_days(days_from_civil(y, m, d) + 1);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

// Howard Hinnant's civil-date algorithms.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_and_dates() {
        assert_eq!(zone(120), "+0200");
        assert_eq!(zone(-330), "-0530");
        assert_eq!(zone(0), "+0000");
        assert_eq!(next_day("2026-02-28").as_deref(), Some("2026-03-01"));
        assert_eq!(next_day("2028-02-28").as_deref(), Some("2028-02-29"));
        assert_eq!(next_day("2026-12-31").as_deref(), Some("2027-01-01"));
        assert_eq!(next_day("nope"), None);
    }

    #[test]
    fn folder_map_names_mailboxes() {
        let map = FolderMap::new(Some('.'), &[(FolderRole::Archive, "Archive")]);
        let path = |p: &str| FolderRef::Path {
            path: p.into(),
            account_id: None,
        };
        assert_eq!(
            map.mailbox(&path("Lists/Rust")).as_deref(),
            Some("Lists.Rust")
        );
        assert_eq!(map.mailbox(&path("a.b")), None);
        assert_eq!(map.mailbox(&path("a//b")), None);
        let role = |r| FolderRef::Role {
            role: r,
            account_id: None,
        };
        assert_eq!(
            map.mailbox(&role(FolderRole::Inbox)).as_deref(),
            Some("INBOX")
        );
        assert_eq!(
            map.mailbox(&role(FolderRole::Archive)).as_deref(),
            Some("Archive")
        );
        assert_eq!(map.mailbox(&role(FolderRole::Trash)), None);
        let slash = FolderMap::default();
        assert_eq!(slash.mailbox(&path("a/b")).as_deref(), Some("a/b"));
    }

    #[test]
    fn keywords_and_addresses() {
        assert!(is_keyword("newsletter") && is_keyword("$Label1"));
        assert!(!is_keyword("two words") && !is_keyword("\\Seen") && !is_keyword("a*"));
        assert!(!is_keyword(""));
        assert!(is_address(" ada@example.org "));
        assert!(!is_address("ada") && !is_address("a b@c") && !is_address("<a@b>"));
    }
}
