//! `rules.kdl`: filter rules (M1b WP22, design.md "Filters & rules").
//!
//! * [`parse_rules`] reads and validates a whole file into
//!   [`nail_api::rules::Rule`]s and reports **every** error it finds (one
//!   per broken rule, condition, action, trigger or scope), each with its
//!   position.
//! * [`parse_rule_text`] / [`rule_to_text`] are the editor's text view of a
//!   single rule (`RuleFromText` / `RuleToText`).
//! * [`RulesDocument`] is the file as the UI edits it: save (create or
//!   replace), delete and reorder rules, then [`RulesDocument::save`]. Rules
//!   the edit does not touch stay byte for byte as they were; inside a
//!   changed rule, the header line, the `when` and `then` blocks and every
//!   condition or action that did not change keep their text and the
//!   comments above them.
//! * [`rules_schema`] is the JSON Schema of the file.
//!
//! ```kdl
//! rule "Newsletters" trigger="arrival" scope="virtual:everything" {
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
//!
//! The format is documented for users in `userDocs/en/rules.md`.
//! Regular expressions and search queries are checked by `nail-rules`
//! (which compiles them); this module checks everything else.

mod read;
mod schema;
mod write;

use std::path::Path;

use nail_api::rules::{Condition, Rule};
use nail_api::{RuleId, SourceLocation};

use crate::error::{ConfigError, ConfigErrorKind};
use crate::kdl_util::Source;
use crate::l10n::msg;

pub use schema::rules_schema;

/// File name of the rules in the config directory.
pub const RULES_FILE: &str = "rules.kdl";

/// File name used for errors of a single rule's text form
/// ([`parse_rule_text`]).
pub const RULE_TEXT_SOURCE: &str = "rule";

/// Longest rule id.
pub const MAX_RULE_ID_LEN: usize = 64;

/// Parses and validates `rules.kdl`. `file` is the name used in error
/// messages (e.g. `"rules.kdl"`). An empty file has no rules. On failure
/// every error found is returned, in file order.
pub fn parse_rules(text: &str, file: &str) -> Result<Vec<Rule>, Vec<ConfigError>> {
    let src = Source::new(file, text);
    let (rules, errors) = read::read(&src);
    if errors.is_empty() {
        Ok(rules)
    } else {
        Err(errors)
    }
}

/// Parses the text form of exactly one rule (`rule "<name>" { … }`), as the
/// editor's text view sends it. Positions refer to `text`; the file name in
/// errors is [`RULE_TEXT_SOURCE`]. Without `id=` the id is derived from the
/// name.
pub fn parse_rule_text(text: &str) -> Result<Rule, Vec<ConfigError>> {
    let src = Source::new(RULE_TEXT_SOURCE, text);
    let (mut rules, mut errors) = read::read(&src);
    if !errors.is_empty() {
        return Err(errors);
    }
    match rules.len() {
        1 => Ok(rules.remove(0)),
        n => {
            let message = msg("config-rule-text-count").arg("count", n);
            errors.push(ConfigError::invalid(&src.loc_at(0), message));
            Err(errors)
        }
    }
}

/// The text form of one rule in the canonical layout (four-space
/// indentation), ending with a newline.
pub fn rule_to_text(rule: &Rule) -> String {
    write::render_rule(&normalize_rule(rule.clone()), "    ")
}

/// A complete `rules.kdl` in the canonical layout.
pub fn render_rules(rules: &[Rule]) -> String {
    write::render_all(rules, "    ")
}

/// Whether `id` is a valid rule id: 1 to 64 characters from
/// `a-z 0-9 - _ .`, starting with a letter or digit.
pub fn is_valid_rule_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_RULE_ID_LEN
        && id
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// The id a rule named `name` gets when its node has no `id=`: the name
/// lowercased, other characters turned into `-`, at most 60 characters,
/// `rule` when nothing is left.
pub fn rule_id_base(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug
        .trim_matches('-')
        .chars()
        .take(MAX_RULE_ID_LEN - 4)
        .collect();
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() {
        "rule".to_owned()
    } else {
        slug
    }
}

/// A free rule id for `name`: [`rule_id_base`], suffixed with `-2`, `-3`, …
/// while `taken`.
pub fn derive_rule_id(name: &str, taken: impl Fn(&str) -> bool) -> RuleId {
    let base = rule_id_base(name);
    if !taken(&base) {
        return RuleId::new(base);
    }
    let mut n: u64 = 2;
    loop {
        let id = format!("{base}-{n}");
        if !taken(&id) {
            return RuleId::new(id);
        }
        n += 1;
    }
}

/// The form a rule has after being written and read back: the conditions'
/// root is an `all` or `any` group (a bare condition `c` becomes
/// `all { c }`), the name is trimmed and the source location is dropped.
pub fn normalize_rule(mut rule: Rule) -> Rule {
    rule.name = rule.name.trim().to_owned();
    rule.source = None;
    if !matches!(
        rule.conditions,
        Condition::All { .. } | Condition::Any { .. }
    ) {
        rule.conditions = Condition::All {
            conditions: vec![rule.conditions],
        };
    }
    rule
}

/// Two rules mean the same (ignoring where they are written).
fn same_rule(a: &Rule, b: &Rule) -> bool {
    normalize_rule(a.clone()) == normalize_rule(b.clone())
}

/// `rules.kdl` as a document the UI edits.
///
/// Holds the file text and its rules. Every edit computes the new text with
/// the format-preserving writer, re-reads it with the validating reader and
/// only then replaces the document, so a `RulesDocument` is always valid and
/// [`Self::text`] always means [`Self::rules`].
#[derive(Debug, Clone, PartialEq)]
pub struct RulesDocument {
    file: String,
    text: String,
    rules: Vec<Rule>,
}

impl RulesDocument {
    /// An empty document (no file yet).
    pub fn empty(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            text: String::new(),
            rules: Vec::new(),
        }
    }

    /// Parses and validates existing file text.
    pub fn parse(text: &str, file: impl Into<String>) -> Result<Self, Vec<ConfigError>> {
        let file = file.into();
        let rules = parse_rules(text, &file)?;
        Ok(Self {
            file,
            text: text.to_owned(),
            rules,
        })
    }

    /// Loads `<dir>/rules.kdl`; a missing file gives an empty document.
    pub fn load(dir: &Path) -> Result<Self, Vec<ConfigError>> {
        match std::fs::read_to_string(dir.join(RULES_FILE)) {
            Ok(text) => Self::parse(&text, RULES_FILE),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty(RULES_FILE)),
            Err(e) => Err(vec![ConfigError::io(RULES_FILE, &e)]),
        }
    }

    /// Writes the document to `<dir>/rules.kdl` atomically (a temporary
    /// file in the same directory, then a rename), creating `dir` if needed.
    pub fn save(&self, dir: &Path) -> Result<(), ConfigError> {
        let io = |e: std::io::Error| ConfigError::io(&self.file, &e);
        std::fs::create_dir_all(dir).map_err(io)?;
        let target = dir.join(RULES_FILE);
        let temp = dir.join(format!(".{RULES_FILE}.tmp"));
        std::fs::write(&temp, self.text.as_bytes()).map_err(io)?;
        std::fs::rename(&temp, &target).map_err(|e| {
            crate::backup::remove_temp(&temp);
            io(e)
        })
    }

    /// The file text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The rules in file order (with their source locations).
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// The rule with this id.
    pub fn rule(&self, id: &RuleId) -> Option<&Rule> {
        self.rules.iter().find(|r| &r.id == id)
    }

    /// Whether a rule uses `id`.
    pub fn is_taken(&self, id: &str) -> bool {
        self.rules.iter().any(|r| r.id.as_str() == id)
    }

    /// A free id for a rule named `name` (see [`derive_rule_id`]).
    pub fn derive_id(&self, name: &str) -> RuleId {
        derive_rule_id(name, |id| self.is_taken(id))
    }

    /// Replaces all rules (their order included), keeping the formatting
    /// of everything that did not change. On error the document is
    /// unchanged; error positions refer to the text the change would have
    /// produced.
    pub fn set_rules(&mut self, rules: Vec<Rule>) -> Result<(), Vec<ConfigError>> {
        let wanted: Vec<Rule> = rules.into_iter().map(normalize_rule).collect();
        if let Some(problem) = self.check_ids(&wanted) {
            return Err(vec![problem]);
        }
        if wanted.len() == self.rules.len()
            && wanted.iter().zip(&self.rules).all(|(a, b)| same_rule(a, b))
        {
            return Ok(());
        }
        let text = write::update_text(&self.text, &self.rules, &wanted);
        let parsed = parse_rules(&text, &self.file)?;
        if !same_list(&parsed, &wanted) {
            // The preserving path could not express the change exactly
            // (should not happen): write the canonical form of the file.
            tracing::warn!("rules.kdl: preserving edit mismatched; writing the canonical form");
            let canonical = write::render_all(&wanted, &write::detect_indent(&self.text));
            let reparsed = parse_rules(&canonical, &self.file)?;
            if !same_list(&reparsed, &wanted) {
                return Err(vec![
                    self.whole_file_error(msg("config-rules-not-writable")),
                ]);
            }
            self.text = canonical;
            self.rules = reparsed;
            return Ok(());
        }
        self.text = text;
        self.rules = parsed;
        Ok(())
    }

    /// Creates or replaces a rule and returns it as saved (with its id and
    /// source location). An empty id creates a new rule with an id derived
    /// from the name, inserted at `position` (0 = first; `None` or past the
    /// end = last). A rule with an unknown non-empty id is created with that
    /// id. An existing rule keeps its place.
    pub fn upsert_rule(
        &mut self,
        rule: Rule,
        position: Option<usize>,
    ) -> Result<Rule, Vec<ConfigError>> {
        let mut rule = normalize_rule(rule);
        if rule.name.is_empty() {
            return Err(vec![
                self.whole_file_error(msg("config-rule-needs-name-value")),
            ]);
        }
        let mut rules = self.rules.clone();
        if rule.id.as_str().is_empty() {
            rule.id = self.derive_id(&rule.name);
        }
        if let Some(existing) = rules.iter_mut().find(|r| r.id == rule.id) {
            *existing = rule.clone();
        } else {
            let at = position.unwrap_or(rules.len()).min(rules.len());
            rules.insert(at, rule.clone());
        }
        self.set_rules(rules)?;
        // The document holds the saved rule (as written back).
        Ok(self.rule(&rule.id).cloned().unwrap_or(rule))
    }

    /// Removes a rule. Returns whether it existed.
    pub fn remove_rule(&mut self, id: &RuleId) -> Result<bool, Vec<ConfigError>> {
        if self.rule(id).is_none() {
            return Ok(false);
        }
        let rules = self.rules.iter().filter(|r| &r.id != id).cloned().collect();
        self.set_rules(rules)?;
        Ok(true)
    }

    /// Puts the rules in the order of `ids`, which must list every rule
    /// exactly once.
    pub fn reorder(&mut self, ids: &[RuleId]) -> Result<(), Vec<ConfigError>> {
        let mut seen = std::collections::HashSet::new();
        let complete = ids.len() == self.rules.len()
            && ids
                .iter()
                .all(|id| seen.insert(id) && self.rule(id).is_some());
        if !complete {
            return Err(vec![
                self.whole_file_error(msg("config-rules-order-incomplete")),
            ]);
        }
        let rules = ids.iter().filter_map(|id| self.rule(id).cloned()).collect();
        self.set_rules(rules)
    }

    fn check_ids(&self, rules: &[Rule]) -> Option<ConfigError> {
        let mut seen = std::collections::HashSet::new();
        for rule in rules {
            if !is_valid_rule_id(rule.id.as_str()) {
                return Some(
                    self.whole_file_error(
                        msg("config-rule-id-invalid").arg("id", rule.id.as_str()),
                    ),
                );
            }
            if !seen.insert(rule.id.as_str()) {
                return Some(self.whole_file_error(
                    msg("config-rule-id-duplicate").arg("id", rule.id.as_str()),
                ));
            }
            if rule.name.is_empty() {
                return Some(self.whole_file_error(msg("config-rule-needs-name-value")));
            }
        }
        None
    }

    fn whole_file_error(&self, message: nail_api::LocalizedMessage) -> ConfigError {
        ConfigError::at(
            ConfigErrorKind::Invalid,
            &SourceLocation {
                file: self.file.clone(),
                line: 1,
                column: 1,
            },
            message,
        )
    }
}

fn same_list(a: &[Rule], b: &[Rule]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_rule(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_ids() {
        for ok in [
            "newsletters",
            "a",
            "0",
            "lists.rust-lang_2",
            &"a".repeat(64),
        ] {
            assert!(is_valid_rule_id(ok), "{ok}");
        }
        for bad in ["", "News", "-a", "a b", "ä", &"a".repeat(65)] {
            assert!(!is_valid_rule_id(bad), "{bad}");
        }
    }

    #[test]
    fn derived_ids() {
        let none = |_: &str| false;
        assert_eq!(derive_rule_id("Newsletters", none).as_str(), "newsletters");
        assert_eq!(
            derive_rule_id("  Rust: users list! ", none).as_str(),
            "rust-users-list"
        );
        assert_eq!(derive_rule_id("äöü", none).as_str(), "rule");
        let taken = |id: &str| id == "x" || id == "x-2";
        assert_eq!(derive_rule_id("X", taken).as_str(), "x-3");
        let long = derive_rule_id(&"y".repeat(200), none);
        assert!(is_valid_rule_id(long.as_str()));
    }

    #[test]
    fn normalizing_wraps_bare_conditions() {
        let rule = Rule {
            id: "a".into(),
            name: " A ".into(),
            enabled: true,
            triggers: vec![],
            scope: vec![],
            conditions: Condition::Tag { tag: "x".into() },
            actions: vec![],
            source: Some(SourceLocation {
                file: "rules.kdl".into(),
                line: 1,
                column: 1,
            }),
        };
        let n = normalize_rule(rule);
        assert_eq!(n.name, "A");
        assert!(n.source.is_none());
        assert_eq!(
            n.conditions,
            Condition::All {
                conditions: vec![Condition::Tag { tag: "x".into() }]
            }
        );
    }
}
